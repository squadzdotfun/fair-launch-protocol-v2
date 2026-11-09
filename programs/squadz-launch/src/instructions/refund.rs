//! Self-serve refunds. The member hands back their coins, the contract sells just enough to return their SOL, and the
//! curve steps back. They pay Meteora's fee both ways and our 0.01 SOL, which a closed lobby skips.
use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use anchor_spl::token::{self, Transfer as SplTransfer};
use anchor_spl::token_2022;

use super::*;
use crate::meteora::{self, swap_ix};
use crate::state::*;

#[derive(Accounts)]
pub struct Refund<'info> {
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
    #[account(mut, close = member, seeds = [MEMBER_SEED, lobby.key().as_ref(), member.key().as_ref()], bump = record.bump, has_one = lobby, constraint = record.wallet == member.key() @ LaunchError::AccountMismatch)]
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
    /// CHECK: the member's wrapped SOL account, which they unwrap in the same transaction
    #[account(mut)]
    pub member_wsol: UncheckedAccount<'info>,
    /// CHECK: checked by seeds, the $SQUADZ burn jar
    #[account(mut, seeds = [BURN_JAR_SEED], bump)]
    pub burn_jar: UncheckedAccount<'info>,
    pub token_program: Program<'info, token::Token>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
    pub system_program: Program<'info, System>,
}

pub fn handle<'info>(ctx: Context<'_, '_, 'info, 'info, Refund<'info>>) -> Result<()> {
    let lobby_key = ctx.accounts.lobby.key();
    let member_key = ctx.accounts.member.key();
    let mint_key = ctx.accounts.lobby.mint;
    require!(ctx.accounts.lobby.state != STATE_LAUNCHED, LaunchError::AlreadyLaunched);

    require_keys_eq!(ctx.accounts.coin_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.wsol_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.member_coins.key(), get_associated_token_address_with_program_id(&member_key, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.member_wsol.key(), get_associated_token_address_with_program_id(&member_key, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);

    let m = ctx.remaining_accounts;
    require!(m.len() >= meteora::SWAP_WITH_HOOK_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::CONFIG].key, ctx.accounts.lobby.config, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::POOL].key, ctx.accounts.lobby.pool, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::INPUT].key, ctx.accounts.coin_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::OUTPUT].key, ctx.accounts.wsol_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::BASE_MINT].key, mint_key, LaunchError::AccountMismatch);
    require_keys_eq!(*m[swap_ix::PAYER].key, lobby_key, LaunchError::AccountMismatch);
    // No referral: passing the program itself means "none" to Meteora.
    require_keys_eq!(*m[swap_ix::REFERRAL].key, METEORA_DBC, LaunchError::AccountMismatch);
    let hook_accounts = &m[meteora::SWAP_LEN..meteora::SWAP_WITH_HOOK_LEN];
    check_hook_accounts(hook_accounts, &ctx.accounts.gate.key(), &mint_key)?;

    let (paid, swap_in, fee_held, donation, coins) = {
        let r = &ctx.accounts.record;
        (r.paid, r.swap_in, r.fee_held, r.donation, r.coins)
    };

    // SOL back from the curve: what this buy added, less Meteora's fee on the way out. Capped for the last refund,
    // which may ask for slightly more than the curve holds after rounding.
    let keep = FEE_DENOMINATOR - CURVE_FEE_NUMERATOR;
    let added = (swap_in as u128) * keep / FEE_DENOMINATOR;
    let reserve = meteora::quote_reserve(&m[swap_ix::POOL])? as u128;
    let gross = added.min(reserve.saturating_sub(RESERVE_ROUNDING_MARGIN));
    let sol_out = (gross * keep / FEE_DENOMINATOR) as u64;

    let bump = ctx.accounts.lobby.bump;
    let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];
    let token_2022_info = ctx.accounts.token_2022_program.to_account_info();

    set_gate(&ctx.accounts.gate, true)?;
    transfer_coins(
        &token_2022_info,
        &ctx.accounts.member_coins.to_account_info(),
        &ctx.accounts.mint.to_account_info(),
        &ctx.accounts.coin_vault.to_account_info(),
        &ctx.accounts.member.to_account_info(),
        hook_accounts,
        coins,
        &[],
    )?;
    if sol_out > 0 {
        let max_in = meteora::token_balance(&ctx.accounts.coin_vault)?;
        meteora::swap(sol_out, max_in, meteora::MODE_EXACT_OUT, m, &lobby_key, &[seeds])?;
    }
    set_gate(&ctx.accounts.gate, false)?;

    if sol_out > 0 {
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                SplTransfer { from: ctx.accounts.wsol_vault.to_account_info(), to: ctx.accounts.member_wsol.to_account_info(), authority: ctx.accounts.lobby.to_account_info() },
                &[seeds],
            ),
            sol_out,
        )?;
    }

    let refund_fee = if ctx.accounts.lobby.state == STATE_CLOSED { 0 } else { REFUND_FEE };
    let from_vault = fee_held + donation;
    let to_member = from_vault.saturating_sub(refund_fee);
    let to_jar = from_vault - to_member;
    let vault_bump = ctx.accounts.lobby.vault_bump;
    let vault_seeds: &[&[u8]] = &[VAULT_SEED, mint_key.as_ref(), &[vault_bump]];
    let sys = ctx.accounts.system_program.to_account_info();
    pay_from_vault(&sys, &ctx.accounts.vault.to_account_info(), &ctx.accounts.member.to_account_info(), to_member, &[vault_seeds])?;
    pay_from_vault(&sys, &ctx.accounts.vault.to_account_info(), &ctx.accounts.burn_jar.to_account_info(), to_jar, &[vault_seeds])?;

    let l = &mut ctx.accounts.lobby;
    l.total_paid -= paid;
    l.launch_fee_held -= fee_held;
    l.donations_total -= donation;
    l.members -= 1;
    let total = l.total_paid;

    emit!(Refunded { lobby: lobby_key, member: member_key, sol_from_curve: sol_out, from_vault: to_member, fee: to_jar, total_paid: total });
    Ok(())
}

#[derive(Accounts)]
pub struct CloseLobby<'info> {
    pub creator: Signer<'info>,
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump, has_one = creator @ LaunchError::NotCreator)]
    pub lobby: Box<Account<'info, Lobby>>,
}

/// A creator closes their lobby early. Members refund without the 0.01 SOL.
pub fn handle_close(ctx: Context<CloseLobby>) -> Result<()> {
    require!(ctx.accounts.lobby.state == STATE_OPEN, LaunchError::NotOpen);
    ctx.accounts.lobby.state = STATE_CLOSED;
    Ok(())
}

#[event]
pub struct Refunded {
    pub lobby: Pubkey,
    pub member: Pubkey,
    pub sol_from_curve: u64,
    pub from_vault: u64,
    pub fee: u64,
    pub total_paid: u64,
}
