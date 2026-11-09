//! A member's buy. It's a real buy on the curve, so the chart moves, but the member gets the flat amount of coins
//! for their SOL. A buy can't be bigger than what's left of the target, so the final buy lands exactly on it and launches
//! the coin. A wallet can buy again (top-ups) until it has put in MAX_PER_WALLET; each buy needs its own pass.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions as ix_sysvar;
use anchor_lang::system_program;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use anchor_spl::token::{self, SyncNative};
use anchor_spl::token_2022;

use super::*;
use crate::meteora::{self, swap_ix};
use crate::pass::verify_pass;
use crate::state::*;

#[derive(Accounts)]
pub struct Buy<'info> {
    #[account(mut)]
    pub member: Signer<'info>,
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: checked by seeds and owner, the hook reads its flag
    #[account(mut, seeds = [GATE_SEED, lobby.mint.as_ref()], bump = lobby.gate_bump, owner = crate::ID)]
    pub gate: UncheckedAccount<'info>,
    /// CHECK: checked by seeds, holds SOL only
    #[account(mut, seeds = [VAULT_SEED, lobby.mint.as_ref()], bump = lobby.vault_bump)]
    pub vault: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = member, space = 8 + Member::INIT_SPACE, seeds = [MEMBER_SEED, lobby.key().as_ref(), member.key().as_ref()], bump)]
    pub record: Box<Account<'info, Member>>,
    /// CHECK: address checked against the lobby
    #[account(address = lobby.mint)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: the lobby's coin account, checked in the handler
    #[account(mut)]
    pub coin_vault: UncheckedAccount<'info>,
    /// CHECK: the lobby's wrapped SOL account, checked in the handler
    #[account(mut)]
    pub wsol_vault: UncheckedAccount<'info>,
    /// CHECK: the member's coin account, checked in the handler
    #[account(mut)]
    pub member_coins: UncheckedAccount<'info>,
    /// CHECK: address checked
    #[account(address = ix_sysvar::ID)]
    pub instructions: UncheckedAccount<'info>,
    pub token_program: Program<'info, token::Token>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
    pub system_program: Program<'info, System>,
}

pub fn handle<'info>(ctx: Context<'_, '_, 'info, 'info, Buy<'info>>, amount: u64, donation: u64, pass_expiry: i64) -> Result<()> {
    buy_core(ctx, amount, donation, Some(pass_expiry))
}

/// The creator's own seat, bought in the same transaction that creates the lobby (founders 9 Nov: create + own buy in
/// one wallet approval). It's the normal buy, without a join pass, so it's tightly limited: only the lobby's creator,
/// only while the lobby is still empty, and only at the moment the lobby was created (same clock reading, so in
/// practice the creating transaction). No donation. Every other rule of a buy applies unchanged.
pub fn creator_seat<'info>(ctx: Context<'_, '_, 'info, 'info, Buy<'info>>, amount: u64) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    require_keys_eq!(ctx.accounts.member.key(), ctx.accounts.lobby.creator, LaunchError::NotCreator);
    require!(ctx.accounts.lobby.total_paid == 0 && ctx.accounts.lobby.members == 0, LaunchError::SeatTooLate);
    require!(now == ctx.accounts.lobby.created_at, LaunchError::SeatTooLate);
    buy_core(ctx, amount, 0, None)
}

/// A buy. `pass_expiry` is the join pass to check (None only for the creator's seat, checked by the caller).
fn buy_core<'info>(ctx: Context<'_, '_, 'info, 'info, Buy<'info>>, amount: u64, donation: u64, pass_expiry: Option<i64>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let lobby_key = ctx.accounts.lobby.key();
    let member_key = ctx.accounts.member.key();
    let mint_key = ctx.accounts.lobby.mint;

    require!(ctx.accounts.lobby.state == STATE_OPEN, LaunchError::NotOpen);
    require!(now < ctx.accounts.lobby.last_buy_at + INACTIVITY_LIMIT, LaunchError::TimedOut);
    require!(BUY_AMOUNTS.contains(&amount), LaunchError::BadAmount);
    // A new record reads paid = 0 (every buy is at least 0.5 SOL). Seeds tie the record to this lobby and wallet.
    let first = ctx.accounts.record.paid == 0;
    let wallet_total = ctx.accounts.record.paid.checked_add(amount).ok_or(LaunchError::Overflow)?;
    require!(wallet_total <= MAX_PER_WALLET, LaunchError::WalletLimit);
    require!(donation == 0 || donation >= MIN_DONATION, LaunchError::BadDonation);
    if let Some(pass_expiry) = pass_expiry {
        require!(now <= pass_expiry, LaunchError::PassExpired);
        verify_pass(&ctx.accounts.instructions, &ctx.accounts.lobby.gatekeeper, &lobby_key, &member_key, pass_expiry)?;
    }

    require_keys_eq!(ctx.accounts.coin_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.wsol_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.member_coins.key(), get_associated_token_address_with_program_id(&member_key, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);

    // Meteora's accounts, then the hook's: gate, hook program, list.
    let m = ctx.remaining_accounts;
    require!(m.len() >= meteora::SWAP_WITH_HOOK_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::CONFIG].key, ctx.accounts.lobby.config, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::POOL].key, ctx.accounts.lobby.pool, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::INPUT].key, ctx.accounts.wsol_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::OUTPUT].key, ctx.accounts.coin_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::BASE_MINT].key, mint_key, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::PAYER].key, lobby_key, LaunchError::AccountMismatch);
    // No referral: passing the program itself means "none" to Meteora.
    require_keys_eq!(*m[swap_ix::REFERRAL].key, METEORA_DBC, LaunchError::AccountMismatch);
    let hook_accounts = &m[meteora::SWAP_LEN..meteora::SWAP_WITH_HOOK_LEN];
    check_hook_accounts(hook_accounts, &ctx.accounts.gate.key(), &mint_key)?;

    // Exact fill: a buy must fit in what's left, and the buy that fills it is the one that launches.
    let new_total = ctx.accounts.lobby.total_paid.checked_add(amount).ok_or(LaunchError::Overflow)?;
    require!(new_total <= ctx.accounts.lobby.target, LaunchError::BuyTooBig);
    let is_final = new_total == ctx.accounts.lobby.target;

    let mut swap_in = amount - amount * LAUNCH_FEE_BPS / BPS;
    if is_final {
        // Rounding across many buys and refunds can leave the curve a few lamports off. The final buy sends what's
        // really needed, and the launch fee takes up the difference, so the curve always completes on this buy.
        let reserve = meteora::quote_reserve(&m[swap_ix::POOL])?;
        let need = ctx.accounts.lobby.threshold.saturating_sub(reserve) as u128;
        let keep = FEE_DENOMINATOR - CURVE_FEE_NUMERATOR;
        let needed_in = need.checked_mul(FEE_DENOMINATOR).ok_or(LaunchError::Overflow)?.div_ceil(keep) as u64;
        swap_in = needed_in.saturating_add(LAST_BUY_MARGIN);
        require!(swap_in <= amount, LaunchError::CurveMismatch);
    }
    let fee = amount - swap_in;

    let sys = ctx.accounts.system_program.to_account_info();
    system_program::transfer(
        CpiContext::new(sys.clone(), system_program::Transfer { from: ctx.accounts.member.to_account_info(), to: ctx.accounts.vault.to_account_info() }),
        fee + donation,
    )?;

    system_program::transfer(
        CpiContext::new(sys, system_program::Transfer { from: ctx.accounts.member.to_account_info(), to: ctx.accounts.wsol_vault.to_account_info() }),
        swap_in,
    )?;
    token::sync_native(CpiContext::new(ctx.accounts.token_program.to_account_info(), SyncNative { account: ctx.accounts.wsol_vault.to_account_info() }))?;

    let bump = ctx.accounts.lobby.bump;
    let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];

    set_gate(&ctx.accounts.gate, true)?;
    meteora::swap(swap_in, 0, meteora::MODE_PARTIAL_FILL, m, &lobby_key, &[seeds])?;

    let coins = ((amount as u128) * (ctx.accounts.lobby.coins_for_target as u128) / (ctx.accounts.lobby.target as u128)) as u64;
    transfer_coins(
        &ctx.accounts.token_2022_program.to_account_info(),
        &ctx.accounts.coin_vault.to_account_info(),
        &ctx.accounts.mint.to_account_info(),
        &ctx.accounts.member_coins.to_account_info(),
        &ctx.accounts.lobby.to_account_info(),
        hook_accounts,
        coins,
        &[seeds],
    )?;
    set_gate(&ctx.accounts.gate, false)?;

    // The curve completes on the final buy and on no other.
    let complete = meteora::quote_reserve(&m[swap_ix::POOL])? >= ctx.accounts.lobby.threshold;
    require!(complete == is_final, LaunchError::CurveMismatch);

    let l = &mut ctx.accounts.lobby;
    l.total_paid += amount;
    l.launch_fee_held += fee;
    l.donations_total += donation;
    if first {
        l.members += 1;
    }
    l.last_buy_at = now;
    if complete {
        l.state = STATE_LAUNCHED;
        l.launched_at = now;
        // Every member starts out eligible, and holder checks can run from this moment.
        l.qualified_paid = l.total_paid;
    }
    let total = l.total_paid;

    let r = &mut ctx.accounts.record;
    if first {
        r.lobby = lobby_key;
        r.wallet = member_key;
        r.out = false;
        r.next_drop = 0;
        r.out_at = 0;
        r.checked_for = 0;
        r.bump = ctx.bumps.record;
    }
    r.paid = wallet_total;
    r.swap_in = r.swap_in.checked_add(swap_in).ok_or(LaunchError::Overflow)?;
    r.fee_held = r.fee_held.checked_add(fee).ok_or(LaunchError::Overflow)?;
    r.donation = r.donation.checked_add(donation).ok_or(LaunchError::Overflow)?;
    r.coins = r.coins.checked_add(coins).ok_or(LaunchError::Overflow)?;

    emit!(Bought { lobby: lobby_key, member: member_key, amount, coins, total_paid: total, complete });
    Ok(())
}

#[event]
pub struct Bought {
    pub lobby: Pubkey,
    pub member: Pubkey,
    pub amount: u64,
    pub coins: u64,
    pub total_paid: u64,
    pub complete: bool,
}
