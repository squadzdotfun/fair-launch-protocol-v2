//! Everything after the curve completes: settling the launch fee, collecting coins that Meteora held outside the curve,
//! HODL drops and holder checks, the treasury, and fee splits.
use anchor_lang::prelude::*;
use anchor_spl::associated_token::get_associated_token_address_with_program_id;
use anchor_spl::token::{self, SyncNative, Transfer as SplTransfer};
use anchor_spl::token_2022::{self, Burn};
use anchor_spl::token_interface::{transfer_checked, TransferChecked};

use super::*;
use crate::meteora::{self, curve_claim_ix, damm_claim_ix, damm_swap_ix};
use crate::state::*;

fn mul_div(a: u64, b: u64, c: u64) -> Result<u64> {
    require!(c > 0, LaunchError::Overflow);
    u64::try_from((a as u128) * (b as u128) / (c as u128)).map_err(|_| error!(LaunchError::Overflow))
}

fn pay_coins<'info>(program: &AccountInfo<'info>, from: &AccountInfo<'info>, mint: &AccountInfo<'info>, to: &AccountInfo<'info>, authority: &AccountInfo<'info>, amount: u64, seeds: &[&[&[u8]]]) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    transfer_checked(
        CpiContext::new_with_signer(program.clone(), TransferChecked { from: from.clone(), mint: mint.clone(), to: to.clone(), authority: authority.clone() }, seeds),
        amount,
        COIN_DECIMALS,
    )
}

/// True if a drop has come due and hasn't released yet.
fn drop_is_due(l: &Lobby, now: i64) -> bool {
    (l.drops_done as usize) < DROP_TIMES.len() && now >= l.launched_at + DROP_TIMES[l.drops_done as usize]
}

/// Marks a member out for good. If they'd already been counted for the drop that's due, that count comes off, so their
/// share goes to the rest.
fn mark_out(l: &mut Lobby, r: &mut Member) {
    r.out = true;
    r.out_at = l.drops_done;
    l.qualified_paid = l.qualified_paid.saturating_sub(r.paid);
    l.members_out += 1;
    if r.checked_for > l.drops_done {
        l.round_checked = l.round_checked.saturating_sub(1);
    }
}

/// Releases the due drop once every member still in has been checked since it came due, as the old contract's round did.
/// Those balances can't all be read at one moment with 300 members, so the round runs in batches. Late bots delay a drop
/// and nobody loses anything.
fn finish_round(l: &mut Lobby, now: i64) {
    let left = l.members.saturating_sub(l.members_out);
    if drop_is_due(l, now) && left > 0 && l.round_checked == left {
        let k = l.drops_done as usize;
        l.qualified_at[k] = l.qualified_paid;
        l.drops_done += 1;
        l.round_checked = 0;
        emit!(DropReleased { mint: l.mint, drop: k as u8, qualified_paid: l.qualified_paid });
    }
}

#[event]
pub struct DropReleased {
    pub mint: Pubkey,
    pub drop: u8,
    pub qualified_paid: u64,
}

// ---- settle ----------------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct SettleLaunch<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: checked by seeds
    #[account(mut, seeds = [VAULT_SEED, lobby.mint.as_ref()], bump = lobby.vault_bump)]
    pub vault: UncheckedAccount<'info>,
    /// CHECK: checked by seeds
    #[account(mut, seeds = [BURN_JAR_SEED], bump)]
    pub burn_jar: UncheckedAccount<'info>,
    /// CHECK: address checked against the lobby. Paid only if it isn't a program, so it can never block settling.
    #[account(address = lobby.donation_wallet)]
    pub donation_wallet: UncheckedAccount<'info>,
    /// CHECK: address checked against the lobby, owner checked when read
    #[account(address = lobby.pool)]
    pub pool: UncheckedAccount<'info>,
    /// CHECK: checked by seeds, receives the cost of the move to the pool
    #[account(mut, seeds = [OPS_SEED], bump)]
    pub ops: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Pays out the launch fee and donations, once the coin has moved to its pool. Anyone can call it, and calling it pays
/// them nothing, so there's no race for it.
pub fn settle(ctx: Context<SettleLaunch>) -> Result<()> {
    require!(ctx.accounts.lobby.state == STATE_LAUNCHED, LaunchError::NotLaunched);
    require!(!ctx.accounts.lobby.settled, LaunchError::AlreadySettled);
    require!(meteora::read_progress(&ctx.accounts.pool)?.migrated, LaunchError::NotMoved);

    let mint_key = ctx.accounts.lobby.mint;
    let vault_bump = ctx.accounts.lobby.vault_bump;
    let seeds: &[&[u8]] = &[VAULT_SEED, mint_key.as_ref(), &[vault_bump]];
    let (fee, donations, treasury_fee) = (ctx.accounts.lobby.launch_fee_held, ctx.accounts.lobby.donations_total, ctx.accounts.lobby.treasury_fee_held);
    let cost = MOVE_TO_POOL_COST.min(fee);
    let sys = ctx.accounts.system_program.to_account_info();
    let vault = ctx.accounts.vault.to_account_info();
    // A wallet that became a program after creation can't receive this way, and would block everything else here. As
    // in the old contract, one bad address blocks nobody. Those donations go to the jar.
    let donations_to_jar = ctx.accounts.donation_wallet.to_account_info().executable;
    let to_jar = fee - cost + treasury_fee + if donations_to_jar { donations } else { 0 };
    pay_from_vault(&sys, &vault, &ctx.accounts.ops.to_account_info(), cost, &[seeds])?;
    if !donations_to_jar {
        pay_from_vault(&sys, &vault, &ctx.accounts.donation_wallet.to_account_info(), donations, &[seeds])?;
    }
    pay_from_vault(&sys, &vault, &ctx.accounts.burn_jar.to_account_info(), to_jar, &[seeds])?;

    let lobby_key = ctx.accounts.lobby.key();
    let l = &mut ctx.accounts.lobby;
    l.settled = true;
    l.launch_fee_held = 0;
    l.donations_total = 0;
    l.treasury_fee_held = 0;
    emit!(Settled { lobby: lobby_key, launch_fee: fee, donations, treasury_fee, donations_to_jar });
    Ok(())
}

#[event]
pub struct Settled {
    pub lobby: Pubkey,
    pub launch_fee: u64,
    pub donations: u64,
    pub treasury_fee: u64,
    pub donations_to_jar: bool,
}

// ---- finalize coins --------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct FinalizeCoins<'info> {
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: address checked against the lobby, owner checked when read
    #[account(address = lobby.pool)]
    pub pool: UncheckedAccount<'info>,
    /// CHECK: address checked against the lobby
    #[account(address = lobby.mint)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub coin_vault: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub reserve_coins: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
}

/// Works out the HODL pot and treasury, once Meteora has sent the coins it held outside the curve. Anyone can call it.
pub fn finalize(ctx: Context<FinalizeCoins>) -> Result<()> {
    let lobby_key = ctx.accounts.lobby.key();
    let mint_key = ctx.accounts.lobby.mint;
    require!(ctx.accounts.lobby.state == STATE_LAUNCHED, LaunchError::NotLaunched);
    require!(!ctx.accounts.lobby.coins_finalized, LaunchError::AlreadySettled);
    require!(meteora::read_progress(&ctx.accounts.pool)?.leftover_withdrawn, LaunchError::NotFinalized);
    let reserve = Pubkey::find_program_address(&[RESERVE_SEED], &crate::ID).0;
    require_keys_eq!(ctx.accounts.coin_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.reserve_coins.key(), get_associated_token_address_with_program_id(&reserve, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);

    // Whatever the curve left above the flat rate joins the reserve.
    let bump = ctx.accounts.lobby.bump;
    let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];
    let spare = meteora::token_balance(&ctx.accounts.coin_vault)?;
    pay_coins(
        &ctx.accounts.token_2022_program.to_account_info(),
        &ctx.accounts.coin_vault.to_account_info(),
        &ctx.accounts.mint.to_account_info(),
        &ctx.accounts.reserve_coins.to_account_info(),
        &ctx.accounts.lobby.to_account_info(),
        spare,
        &[seeds],
    )?;
    let total = meteora::token_balance(&ctx.accounts.reserve_coins)?;

    let l = &mut ctx.accounts.lobby;
    let treasury = mul_div(SUPPLY, l.treasury_bps as u64, BPS)?;
    l.treasury_total = treasury;
    l.hodl_pot = total.checked_sub(treasury).ok_or(LaunchError::Overflow)?;
    l.coins_finalized = true;
    emit!(CoinsFinalized { lobby: lobby_key, hodl_pot: l.hodl_pot, treasury });
    Ok(())
}

#[event]
pub struct CoinsFinalized {
    pub lobby: Pubkey,
    pub hodl_pot: u64,
    pub treasury: u64,
}

// ---- member claims ---------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct MemberClaim<'info> {
    #[account(mut)]
    pub member: Signer<'info>,
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    #[account(mut, seeds = [MEMBER_SEED, lobby.key().as_ref(), member.key().as_ref()], bump = record.bump, has_one = lobby)]
    pub record: Box<Account<'info, Member>>,
    /// CHECK: address checked against the lobby
    #[account(address = lobby.mint)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub member_coins: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub reserve_coins: UncheckedAccount<'info>,
    /// CHECK: checked by seeds
    #[account(seeds = [RESERVE_SEED], bump)]
    pub reserve: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
}

fn check_claim_accounts(ctx: &Context<MemberClaim>) -> Result<()> {
    let (member_key, mint_key) = (ctx.accounts.member.key(), ctx.accounts.lobby.mint);
    require!(ctx.accounts.lobby.state == STATE_LAUNCHED, LaunchError::NotLaunched);
    require!(ctx.accounts.lobby.coins_finalized, LaunchError::NotFinalized);
    require_keys_eq!(ctx.accounts.member_coins.key(), get_associated_token_address_with_program_id(&member_key, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.reserve_coins.key(), get_associated_token_address_with_program_id(&ctx.accounts.reserve.key(), &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    Ok(())
}

/// A member claims every drop that has released to them. Under 75% of the coins they bought, they're marked out for good.
/// They keep the drops already released, since a full check passed them for each one, and the rest go to remaining holders.
pub fn claim_drops(ctx: Context<MemberClaim>) -> Result<()> {
    check_claim_accounts(&ctx)?;
    let now = Clock::get()?.unix_timestamp;

    let mut marked_now = false;
    if !ctx.accounts.record.out {
        let held = meteora::held_coins(&ctx.accounts.member_coins);
        let line = mul_div(ctx.accounts.record.coins, HOLD_LINE_BPS, BPS)?;
        if held < line {
            mark_out(&mut ctx.accounts.lobby, &mut ctx.accounts.record);
            finish_round(&mut ctx.accounts.lobby, now);
            let lobby_key = ctx.accounts.lobby.key();
            emit!(MemberOut { lobby: lobby_key, wallet: ctx.accounts.member.key(), held, line });
            marked_now = true;
        }
    }

    let limit = if ctx.accounts.record.out { ctx.accounts.record.out_at } else { ctx.accounts.lobby.drops_done };
    let from = ctx.accounts.record.next_drop;
    let mut owed = 0u64;
    let (pot, paid) = (ctx.accounts.lobby.hodl_pot, ctx.accounts.record.paid);
    for k in from..limit {
        let amount = mul_div(pot, DROP_BPS[k as usize], BPS)?;
        let q = ctx.accounts.lobby.qualified_at[k as usize];
        if q > 0 {
            owed = owed.checked_add(mul_div(amount, paid, q)?).ok_or(LaunchError::Overflow)?;
        }
    }
    // A share can round to zero, which still has to count as done, or the record could never close.
    require!(limit > from || marked_now, LaunchError::NothingToDo);
    // Someone who closed their coin account can't be paid yet. The mark still sticks, and what they've earned waits.
    if owed > 0 && ctx.accounts.member_coins.data_is_empty() {
        return Ok(());
    }
    ctx.accounts.record.next_drop = limit;
    let reserve_seeds: &[&[u8]] = &[RESERVE_SEED, &[ctx.bumps.reserve]];
    pay_coins(
        &ctx.accounts.token_2022_program.to_account_info(),
        &ctx.accounts.reserve_coins.to_account_info(),
        &ctx.accounts.mint.to_account_info(),
        &ctx.accounts.member_coins.to_account_info(),
        &ctx.accounts.reserve.to_account_info(),
        owed,
        &[reserve_seeds],
    )?;
    emit!(DropsClaimed { lobby: ctx.accounts.lobby.key(), wallet: ctx.accounts.member.key(), coins: owed, through: limit });
    Ok(())
}

#[event]
pub struct DropsClaimed {
    pub lobby: Pubkey,
    pub wallet: Pubkey,
    pub coins: u64,
    pub through: u8,
}

#[derive(Accounts)]
pub struct BurnUnearned<'info> {
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: address checked against the lobby
    #[account(mut, address = lobby.mint)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub reserve_coins: UncheckedAccount<'info>,
    /// CHECK: checked by seeds, signs for the burn
    #[account(seeds = [RESERVE_SEED], bump)]
    pub reserve: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
}

/// Once nobody is eligible, burns the drops nobody earned. Anyone can call it, and it only runs once. Being out is
/// permanent, so nobody can become eligible again. Drops that were already earned stay claimable.
pub fn burn_unearned_drops(ctx: Context<BurnUnearned>) -> Result<()> {
    let mint_key = ctx.accounts.lobby.mint;
    require!(ctx.accounts.lobby.coins_finalized, LaunchError::NotFinalized);
    require!(ctx.accounts.lobby.qualified_paid == 0, LaunchError::StillEligible);
    require!(!ctx.accounts.lobby.unearned_burned, LaunchError::NothingToDo);
    require_keys_eq!(ctx.accounts.reserve_coins.key(), get_associated_token_address_with_program_id(&ctx.accounts.reserve.key(), &mint_key, &token_2022::ID), LaunchError::AccountMismatch);

    // Worked out per drop, never from the reserve's balance, which also holds the treasury and unclaimed drops.
    let (done, pot) = (ctx.accounts.lobby.drops_done as usize, ctx.accounts.lobby.hodl_pot);
    let mut amount = 0u64;
    for k in 0..DROP_BPS.len() {
        if k >= done || ctx.accounts.lobby.qualified_at[k] == 0 {
            amount = amount.checked_add(mul_div(pot, DROP_BPS[k], BPS)?).ok_or(LaunchError::Overflow)?;
        }
    }
    require!(amount > 0, LaunchError::NothingToDo);
    let seeds: &[&[u8]] = &[RESERVE_SEED, &[ctx.bumps.reserve]];
    token_2022::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_2022_program.to_account_info(),
            Burn { mint: ctx.accounts.mint.to_account_info(), from: ctx.accounts.reserve_coins.to_account_info(), authority: ctx.accounts.reserve.to_account_info() },
            &[seeds],
        ),
        amount,
    )?;
    ctx.accounts.lobby.unearned_burned = true;
    emit!(UnearnedBurned { lobby: ctx.accounts.lobby.key(), coins: amount });
    Ok(())
}

#[event]
pub struct UnearnedBurned {
    pub lobby: Pubkey,
    pub coins: u64,
}

// ---- holder checks ---------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct SpotCheck<'info> {
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
}

/// Anyone can run this, from the moment the coin launches, as in the old contract. It doesn't wait for the move to the
/// pool or the HODL pot, because who moves the coin and when isn't up to us. `remaining_accounts`: pairs of member record
/// and that member's coin account. The contract reads the real balance itself, so a caller can't wrongly remove anyone.
/// One reading under 75% is out for good. While a drop is due, a passing reading counts toward releasing it.
pub fn spot_check<'info>(ctx: Context<'_, '_, 'info, 'info, SpotCheck<'info>>) -> Result<()> {
    let lobby_key = ctx.accounts.lobby.key();
    let mint_key = ctx.accounts.lobby.mint;
    require!(ctx.accounts.lobby.state == STATE_LAUNCHED, LaunchError::NotLaunched);
    let now = Clock::get()?.unix_timestamp;
    require!(!ctx.remaining_accounts.is_empty() && ctx.remaining_accounts.len() % 2 == 0, LaunchError::AccountMismatch);
    for pair in ctx.remaining_accounts.chunks(2) {
        let (rec, coins) = (&pair[0], &pair[1]);
        // A member who's out can close their record, and that mustn't fail everyone else's checks in this batch.
        if rec.data_is_empty() && *rec.owner == anchor_lang::system_program::ID {
            continue;
        }
        require_keys_eq!(*rec.owner, crate::ID, LaunchError::AccountMismatch);
        let mut r = {
            let d = rec.try_borrow_data()?;
            Member::try_deserialize(&mut &d[..])?
        };
        require_keys_eq!(r.lobby, lobby_key, LaunchError::AccountMismatch);
        require_keys_eq!(*coins.key, get_associated_token_address_with_program_id(&r.wallet, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
        if r.out {
            continue;
        }
        // A closed coin account reads as zero.
        let held = meteora::held_coins(coins);
        let line = mul_div(r.coins, HOLD_LINE_BPS, BPS)?;
        let l = &mut ctx.accounts.lobby;
        let done = l.drops_done;
        let changed = if held < line {
            mark_out(l, &mut r);
            emit!(MemberOut { lobby: lobby_key, wallet: r.wallet, held, line });
            true
        } else if drop_is_due(l, now) && r.checked_for <= done {
            r.checked_for = done + 1;
            l.round_checked += 1;
            true
        } else {
            false
        };
        if changed {
            {
                let mut d = rec.try_borrow_mut_data()?;
                let mut w: &mut [u8] = &mut d[..];
                r.try_serialize(&mut w)?;
            }
            finish_round(&mut ctx.accounts.lobby, now);
        }
    }
    Ok(())
}

#[event]
pub struct MemberOut {
    pub lobby: Pubkey,
    pub wallet: Pubkey,
    pub held: u64,
    pub line: u64,
}

// ---- treasury --------------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct ReleaseTreasury<'info> {
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: address checked against the lobby
    #[account(address = lobby.mint)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub reserve_coins: UncheckedAccount<'info>,
    /// CHECK: checked by seeds
    #[account(seeds = [RESERVE_SEED], bump)]
    pub reserve: UncheckedAccount<'info>,
    /// CHECK: the treasury wallet's coin account, checked in the handler
    #[account(mut)]
    pub treasury_coins: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
}

/// 10% at launch, then 30% each at 30, 60 and 90 days. Anyone can call it, and it only ever pays the chosen wallet.
pub fn release_treasury(ctx: Context<ReleaseTreasury>) -> Result<()> {
    let mint_key = ctx.accounts.lobby.mint;
    require!(ctx.accounts.lobby.coins_finalized, LaunchError::NotFinalized);
    require_keys_eq!(ctx.accounts.treasury_coins.key(), get_associated_token_address_with_program_id(&ctx.accounts.lobby.treasury_wallet, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.reserve_coins.key(), get_associated_token_address_with_program_id(&ctx.accounts.reserve.key(), &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    let now = Clock::get()?.unix_timestamp;
    let (launched, total, released) = (ctx.accounts.lobby.launched_at, ctx.accounts.lobby.treasury_total, ctx.accounts.lobby.treasury_released);
    let mut unlocked_bps = 0u64;
    for (i, t) in TREASURY_TIMES.iter().enumerate() {
        if now >= launched + *t {
            unlocked_bps += TREASURY_RELEASE_BPS[i];
        }
    }
    let unlocked = mul_div(total, unlocked_bps, BPS)?;
    let due = unlocked.saturating_sub(released);
    require!(due > 0, LaunchError::TooEarly);
    let reserve_seeds: &[&[u8]] = &[RESERVE_SEED, &[ctx.bumps.reserve]];
    pay_coins(
        &ctx.accounts.token_2022_program.to_account_info(),
        &ctx.accounts.reserve_coins.to_account_info(),
        &ctx.accounts.mint.to_account_info(),
        &ctx.accounts.treasury_coins.to_account_info(),
        &ctx.accounts.reserve.to_account_info(),
        due,
        &[reserve_seeds],
    )?;
    ctx.accounts.lobby.treasury_released = released + due;
    emit!(TreasuryReleased { lobby: ctx.accounts.lobby.key(), coins: due, total_released: released + due });
    Ok(())
}

#[event]
pub struct TreasuryReleased {
    pub lobby: Pubkey,
    pub coins: u64,
    pub total_released: u64,
}

#[derive(Accounts)]
pub struct ReclaimTreasuryFee<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump, has_one = creator @ LaunchError::NotCreator)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: checked by seeds
    #[account(mut, seeds = [VAULT_SEED, lobby.mint.as_ref()], bump = lobby.vault_bump)]
    pub vault: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// A closed or timed-out lobby can never launch, so the creator takes the treasury fee back.
pub fn reclaim_treasury_fee(ctx: Context<ReclaimTreasuryFee>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let l = &ctx.accounts.lobby;
    require!(l.is_over(now), LaunchError::NotClosed);
    let fee = l.treasury_fee_held;
    require!(fee > 0, LaunchError::NothingToDo);
    let mint_key = l.mint;
    let vault_bump = l.vault_bump;
    let seeds: &[&[u8]] = &[VAULT_SEED, mint_key.as_ref(), &[vault_bump]];
    pay_from_vault(&ctx.accounts.system_program.to_account_info(), &ctx.accounts.vault.to_account_info(), &ctx.accounts.creator.to_account_info(), fee, &[seeds])?;
    let l = &mut ctx.accounts.lobby;
    l.treasury_fee_held = 0;
    if l.state == STATE_OPEN {
        l.state = STATE_CLOSED;
    }
    Ok(())
}

// ---- closing a record ------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct CloseRecord<'info> {
    #[account(mut)]
    pub member: Signer<'info>,
    #[account(seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    #[account(mut, close = member, seeds = [MEMBER_SEED, lobby.key().as_ref(), member.key().as_ref()], bump = record.bump, has_one = lobby)]
    pub record: Box<Account<'info, Member>>,
}

/// The member takes their record's deposit back, once it has nothing left to do. A member marked out must claim what
/// they earned first, so closing can't silently lose it.
pub fn close_record(ctx: Context<CloseRecord>) -> Result<()> {
    let r = &ctx.accounts.record;
    require!(ctx.accounts.lobby.state == STATE_LAUNCHED, LaunchError::NotLaunched);
    let last = if r.out { r.out_at as usize } else { DROP_TIMES.len() };
    require!(r.next_drop as usize >= last, LaunchError::RecordInUse);
    Ok(())
}

// ---- upkeep money ----------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct WithdrawOps<'info> {
    /// CHECK: checked by seeds
    #[account(mut, seeds = [OPS_SEED], bump)]
    pub ops: UncheckedAccount<'info>,
    /// CHECK: the one fixed wallet
    #[account(mut, address = OPS_WALLET)]
    pub ops_wallet: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Sends creation fees, approval fees and move allowances to the one fixed wallet. Anyone can call it, and it can pay nowhere else.
pub fn withdraw_ops(ctx: Context<WithdrawOps>) -> Result<()> {
    let keep = Rent::get()?.minimum_balance(0);
    let amount = ctx.accounts.ops.lamports().saturating_sub(keep);
    require!(amount > 0, LaunchError::NothingToDo);
    let seeds: &[&[u8]] = &[OPS_SEED, &[ctx.bumps.ops]];
    system_transfer_signed(&ctx.accounts.system_program.to_account_info(), &ctx.accounts.ops.to_account_info(), &ctx.accounts.ops_wallet.to_account_info(), amount, &[seeds])
}

// ---- fees ------------------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct CollectFees<'info> {
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub wsol_vault: UncheckedAccount<'info>,
    /// CHECK: the $SQUADZ burn jar's wrapped SOL account, checked in the handler
    #[account(mut)]
    pub jar_wsol: UncheckedAccount<'info>,
    /// CHECK: checked by seeds
    #[account(seeds = [BURN_JAR_SEED], bump)]
    pub burn_jar: UncheckedAccount<'info>,
    pub token_program: Program<'info, token::Token>,
}

fn check_fee_accounts(ctx: &Context<CollectFees>) -> Result<()> {
    let lobby_key = ctx.accounts.lobby.key();
    require_keys_eq!(ctx.accounts.wsol_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.jar_wsol.key(), get_associated_token_address_with_program_id(&ctx.accounts.burn_jar.key(), &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    Ok(())
}

fn split_fees<'info>(ctx: &mut Context<'_, '_, 'info, 'info, CollectFees<'info>>, new: u64, burn_bps: u64) -> Result<()> {
    if new == 0 {
        return Ok(());
    }
    let mint_key = ctx.accounts.lobby.mint;
    let bump = ctx.accounts.lobby.bump;
    let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];
    let to_burn = mul_div(new, burn_bps, BPS)?;
    let rest = new - to_burn;
    let move_wsol = |to: AccountInfo<'info>, amount: u64| -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                SplTransfer { from: ctx.accounts.wsol_vault.to_account_info(), to, authority: ctx.accounts.lobby.to_account_info() },
                &[seeds],
            ),
            amount,
        )
    };
    move_wsol(ctx.accounts.jar_wsol.to_account_info(), to_burn)?;
    // A creator's share stays in this account until pay_creator_fees sends it, so a closed account can't block the jar's share.
    if ctx.accounts.lobby.fee_choice == FEES_TO_COIN_BURN {
        if ctx.accounts.lobby.state == STATE_LAUNCHED {
            ctx.accounts.lobby.coin_burn_sol += rest;
        } else {
            // A lobby that's over will never have a pool to burn through, so its burn share goes to the jar.
            move_wsol(ctx.accounts.jar_wsol.to_account_info(), rest)?;
        }
    }
    emit!(FeesSplit { lobby: ctx.accounts.lobby.key(), total: new, to_squadz_burn: to_burn, rest });
    Ok(())
}

#[derive(Accounts)]
pub struct PayCreatorFees<'info> {
    #[account(seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub wsol_vault: UncheckedAccount<'info>,
    /// CHECK: the fee wallet's wrapped SOL account, checked in the handler
    #[account(mut)]
    pub fee_wsol: UncheckedAccount<'info>,
    pub token_program: Program<'info, token::Token>,
}

/// Sends a creator's fee share to the fee wallet. Anyone can call it, and it can pay nowhere else. As in the old contract,
/// a wallet that can't receive is skipped and stays owed, so calling this alongside a collection can't undo the collection.
/// On a lobby whose fees go to the creator, this account's balance is the creator's: the rest was split off when collected.
/// It also holds the final buy's leftover of about 1,000 smallest units, which goes with it. It holds no member money
/// between transactions, because buys and refunds use up what they put in.
pub fn pay_creator_fees(ctx: Context<PayCreatorFees>) -> Result<()> {
    let lobby_key = ctx.accounts.lobby.key();
    require!(ctx.accounts.lobby.fee_choice == FEES_TO_CREATOR, LaunchError::WrongFeeChoice);
    require_keys_eq!(ctx.accounts.wsol_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.fee_wsol.key(), get_associated_token_address_with_program_id(&ctx.accounts.lobby.fee_wallet, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    let amount = meteora::token_balance(&ctx.accounts.wsol_vault)?;
    if amount == 0 {
        return Ok(());
    }
    if *ctx.accounts.fee_wsol.owner != token::ID || ctx.accounts.fee_wsol.data_is_empty() {
        emit!(CreatorFeesWaiting { lobby: lobby_key, amount });
        return Ok(());
    }
    let mint_key = ctx.accounts.lobby.mint;
    let bump = ctx.accounts.lobby.bump;
    let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            SplTransfer { from: ctx.accounts.wsol_vault.to_account_info(), to: ctx.accounts.fee_wsol.to_account_info(), authority: ctx.accounts.lobby.to_account_info() },
            &[seeds],
        ),
        amount,
    )?;
    emit!(CreatorFeesPaid { lobby: lobby_key, amount });
    Ok(())
}

#[event]
pub struct CreatorFeesPaid {
    pub lobby: Pubkey,
    pub amount: u64,
}

/// The fee wallet's wrapped SOL account is closed, so the amount stays in the lobby's account until it's reopened.
#[event]
pub struct CreatorFeesWaiting {
    pub lobby: Pubkey,
    pub amount: u64,
}

/// Curve fees all go to the $SQUADZ burn jar, once a lobby has launched or is over, so a failed lobby never pays the
/// creator. Anyone can call it.
pub fn collect_curve_fees<'info>(mut ctx: Context<'_, '_, 'info, 'info, CollectFees<'info>>) -> Result<()> {
    check_fee_accounts(&ctx)?;
    let now = Clock::get()?.unix_timestamp;
    require!(ctx.accounts.lobby.state == STATE_LAUNCHED || ctx.accounts.lobby.is_over(now), LaunchError::NotClosed);
    let m = ctx.remaining_accounts;
    let lobby_key = ctx.accounts.lobby.key();
    require!(m.len() >= meteora::CURVE_CLAIM_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*m[curve_claim_ix::POOL].key, ctx.accounts.lobby.pool, LaunchError::AccountMismatch);
    require_keys_eq!(*m[curve_claim_ix::TOKEN_B].key, ctx.accounts.wsol_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[curve_claim_ix::CREATOR].key, lobby_key, LaunchError::AccountMismatch);
    let mint_key = ctx.accounts.lobby.mint;
    let bump = ctx.accounts.lobby.bump;
    let before = meteora::token_balance(&ctx.accounts.wsol_vault)?;
    {
        let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];
        meteora::claim_curve_fees(m, &lobby_key, &[seeds])?;
    }
    let new = meteora::token_balance(&ctx.accounts.wsol_vault)?.saturating_sub(before);
    split_fees(&mut ctx, new, BPS)
}

/// Pool fees. Split 50/50 until the first collection after the opening window from the lobby's real share of the pool,
/// then 10/90. A worthless share a stranger sends the lobby can be collected, but it can't end the split, so it can't cut
/// the burn's share of the real fees. After OPENING_SPLIT_LIMIT, anything that isn't the real share splits 10/90, so nobody
/// can keep 50/50 on for good by locking more of the pool than the lobby holds. Anyone can call it.
pub fn collect_pool_fees<'info>(mut ctx: Context<'_, '_, 'info, 'info, CollectFees<'info>>) -> Result<()> {
    check_fee_accounts(&ctx)?;
    // Meteora's accounts, then the settings account that coin's pool came from.
    let (settings, m) = ctx.remaining_accounts.split_last().ok_or(error!(LaunchError::AccountMismatch))?;
    let lobby_key = ctx.accounts.lobby.key();
    let mint_key = ctx.accounts.lobby.mint;
    require!(m.len() >= meteora::DAMM_CLAIM_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_claim_ix::TOKEN_B].key, ctx.accounts.wsol_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_claim_ix::OWNER].key, lobby_key, LaunchError::AccountMismatch);
    // Only the official pool: a stranger's pool could otherwise fake its start time and end the 50/50 split early.
    require_keys_eq!(*m[damm_claim_ix::POOL].key, meteora::official_pool(&mint_key, settings)?, LaunchError::AccountMismatch);
    let pool = meteora::read_damm_pool(&m[damm_claim_ix::POOL])?;
    require_keys_eq!(pool.token_a_mint, mint_key, LaunchError::AccountMismatch);
    let now = Clock::get()?.unix_timestamp;
    let opened = pool.activation_point as i64;
    let real_position = meteora::is_main_locked_position(&m[damm_claim_ix::POSITION], &m[damm_claim_ix::POOL])?;

    let bump = ctx.accounts.lobby.bump;
    let before = meteora::token_balance(&ctx.accounts.wsol_vault)?;
    {
        let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];
        meteora::claim_pool_fees(m, &lobby_key, &[seeds])?;
    }
    let new = meteora::token_balance(&ctx.accounts.wsol_vault)?.saturating_sub(before);

    let limit_passed = now >= opened + OPENING_WINDOW + OPENING_SPLIT_LIMIT;
    let burn_bps = if ctx.accounts.lobby.opening_done || (!real_position && limit_passed) { NORMAL_BURN_BPS } else { OPENING_BURN_BPS };
    {
        let l = &mut ctx.accounts.lobby;
        l.pool_opened_at = opened;
        if real_position && now >= opened + OPENING_WINDOW {
            l.opening_done = true;
        }
    }
    split_fees(&mut ctx, new, burn_bps)
}

#[event]
pub struct FeesSplit {
    pub lobby: Pubkey,
    pub total: u64,
    pub to_squadz_burn: u64,
    pub rest: u64,
}

// ---- buy and burn ----------------------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct CoinBurn<'info> {
    #[account(mut, seeds = [LOBBY_SEED, lobby.mint.as_ref()], bump = lobby.bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    /// CHECK: address checked against the lobby
    #[account(mut, address = lobby.mint)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub coin_vault: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub wsol_vault: UncheckedAccount<'info>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
}

/// The burn's price limit: 2%, or 11% once no burn has succeeded for BURN_STUCK_AFTER, so it can't get stuck for good
/// if the pool's fee is high. Before the first burn the clock runs from launch.
fn burn_slippage_bps(last_burn_at: i64, launched_at: i64, now: i64) -> u64 {
    let since = if last_burn_at != 0 { last_burn_at } else { launched_at };
    if since != 0 && now >= since.saturating_add(BURN_STUCK_AFTER) {
        BURN_STUCK_SLIPPAGE_BPS
    } else {
        BURN_MAX_SLIPPAGE_BPS
    }
}

/// Checks the swap accounts point at the coin's official pool. None if that pool doesn't exist yet or is paused, in
/// which case the money just waits.
fn check_burn_pool(m: &[AccountInfo], settings: &AccountInfo, mint: &Pubkey) -> Result<Option<meteora::DammPool>> {
    require!(m.len() >= meteora::DAMM_SWAP_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::POOL].key, meteora::official_pool(mint, settings)?, LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::MINT_A].key, *mint, LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::MINT_B].key, WSOL_MINT, LaunchError::AccountMismatch);
    // No referral: passing the program itself means "none" to Meteora.
    require_keys_eq!(*m[damm_swap_ix::REFERRAL].key, METEORA_DAMM_V2, LaunchError::AccountMismatch);
    let info = &m[damm_swap_ix::POOL];
    if *info.owner != METEORA_DAMM_V2 || info.data_is_empty() {
        return Ok(None);
    }
    let pool = meteora::read_damm_pool(info)?;
    require_keys_eq!(pool.token_a_mint, *mint, LaunchError::AccountMismatch);
    require_keys_eq!(pool.token_b_mint, WSOL_MINT, LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::VAULT_A].key, pool.token_a_vault, LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::VAULT_B].key, pool.token_b_vault, LaunchError::AccountMismatch);
    Ok(if pool.trading { Some(pool) } else { None })
}

/// At most 0.5% of the SOL behind the pool's permanently locked liquidity. A small size is what makes pushing the price
/// around a burn lose money: the price can be moved in the same transaction, so the price limit alone doesn't stop it.
/// It can't come from the pool's SOL vault, because anyone can add temporary liquidity in that same transaction to raise
/// the vault and take most of a large burn. Locked liquidity can't be added and taken back, and pushing the price still
/// costs more in fees than the bigger burn pays. The old contract has that weakness: `burn.rs:265-267`.
fn burn_size(pool: &meteora::DammPool, available: u64) -> Result<u64> {
    Ok(available.min(mul_div(meteora::locked_sol(pool), BURN_MAX_POOL_BPS, BPS)?))
}

/// Spends part of the SOL set aside for this coin's own buy and burn, at most every 15 minutes (BURN_INTERVAL). Anyone can call
/// it. The size comes from the pool, not the caller, so a caller can't use up the interval on a tiny burn.
pub fn coin_buy_and_burn<'info>(ctx: Context<'_, '_, 'info, 'info, CoinBurn<'info>>) -> Result<()> {
    let lobby_key = ctx.accounts.lobby.key();
    let mint_key = ctx.accounts.lobby.mint;
    let now = Clock::get()?.unix_timestamp;
    require!(ctx.accounts.lobby.fee_choice == FEES_TO_COIN_BURN, LaunchError::WrongFeeChoice);
    require!(ctx.accounts.lobby.state == STATE_LAUNCHED, LaunchError::NotLaunched);
    let set_aside = ctx.accounts.lobby.coin_burn_sol;
    require!(set_aside > 0, LaunchError::NothingToDo);
    require!(now >= ctx.accounts.lobby.last_burn_at + BURN_INTERVAL, LaunchError::TooEarly);
    require_keys_eq!(ctx.accounts.coin_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &mint_key, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.wsol_vault.key(), get_associated_token_address_with_program_id(&lobby_key, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    // Meteora's accounts, then the settings account that coin's pool came from.
    let (settings, m) = ctx.remaining_accounts.split_last().ok_or(error!(LaunchError::AccountMismatch))?;
    let Some(pool) = check_burn_pool(m, settings, &mint_key)? else {
        emit!(BurnSkipped { mint: mint_key, reason: 0 });
        return Ok(());
    };
    require_keys_eq!(*m[damm_swap_ix::INPUT].key, ctx.accounts.wsol_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::OUTPUT].key, ctx.accounts.coin_vault.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::PAYER].key, lobby_key, LaunchError::AccountMismatch);

    let amount = burn_size(&pool, set_aside)?;
    let ideal = meteora::tokens_for_sol(amount, pool.sqrt_price).unwrap_or(0);
    if amount < BURN_MIN_AMOUNT || ideal == 0 {
        emit!(BurnSkipped { mint: mint_key, reason: 1 });
        return Ok(());
    }
    let slippage = burn_slippage_bps(ctx.accounts.lobby.last_burn_at, ctx.accounts.lobby.launched_at, now);
    let min_out = mul_div(ideal, BPS - slippage, BPS)?;

    let bump = ctx.accounts.lobby.bump;
    let seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[bump]];
    let before = meteora::token_balance(&ctx.accounts.coin_vault)?;
    meteora::damm_swap(amount, min_out, m, &lobby_key, &[seeds])?;
    let bought = meteora::token_balance(&ctx.accounts.coin_vault)?.saturating_sub(before);
    require!(bought >= min_out, LaunchError::TooFewCoins);
    token_2022::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_2022_program.to_account_info(),
            Burn { mint: ctx.accounts.mint.to_account_info(), from: ctx.accounts.coin_vault.to_account_info(), authority: ctx.accounts.lobby.to_account_info() },
            &[seeds],
        ),
        bought,
    )?;
    let l = &mut ctx.accounts.lobby;
    l.coin_burn_sol = set_aside - amount;
    l.last_burn_at = now;
    emit!(Burned { mint: mint_key, sol: amount, coins: bought });
    Ok(())
}

#[derive(Accounts)]
pub struct JarBurn<'info> {
    /// Whoever runs the burn. Receives the tip.
    #[account(mut)]
    pub caller: Signer<'info>,
    #[account(init_if_needed, payer = caller, space = 8 + BurnState::INIT_SPACE, seeds = [BURN_STATE_SEED], bump)]
    pub burn_state: Box<Account<'info, BurnState>>,
    /// $SQUADZ's own lobby, for its launch time.
    #[account(seeds = [LOBBY_SEED, SQUADZ_MINT.as_ref()], bump = squadz_lobby.bump)]
    pub squadz_lobby: Box<Account<'info, Lobby>>,
    /// CHECK: checked by seeds, signs for the swap and burn
    #[account(mut, seeds = [BURN_JAR_SEED], bump)]
    pub burn_jar: UncheckedAccount<'info>,
    /// CHECK: address checked
    #[account(mut, address = SQUADZ_MINT)]
    pub mint: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub jar_coins: UncheckedAccount<'info>,
    /// CHECK: checked in the handler
    #[account(mut)]
    pub jar_wsol: UncheckedAccount<'info>,
    pub token_program: Program<'info, token::Token>,
    pub token_2022_program: Program<'info, token_2022::Token2022>,
    pub system_program: Program<'info, System>,
}

/// The burn jar buys and burns $SQUADZ, at most every 15 minutes (JAR_BURN_INTERVAL). Anyone can call it, and gets a
/// small tip. The size comes from the pool, so a caller can't use up the interval on a tiny burn. The jar's money sits as SOL and as wrapped SOL.
pub fn jar_buy_and_burn<'info>(ctx: Context<'_, '_, 'info, 'info, JarBurn<'info>>) -> Result<()> {
    let jar_key = ctx.accounts.burn_jar.key();
    let now = Clock::get()?.unix_timestamp;
    require!(now >= ctx.accounts.burn_state.last_burn_at + JAR_BURN_INTERVAL, LaunchError::TooEarly);
    require_keys_eq!(ctx.accounts.jar_coins.key(), get_associated_token_address_with_program_id(&jar_key, &SQUADZ_MINT, &token_2022::ID), LaunchError::AccountMismatch);
    require_keys_eq!(ctx.accounts.jar_wsol.key(), get_associated_token_address_with_program_id(&jar_key, &WSOL_MINT, &token::ID), LaunchError::AccountMismatch);
    // Meteora's accounts, then the settings account that coin's pool came from.
    let (settings, m) = ctx.remaining_accounts.split_last().ok_or(error!(LaunchError::AccountMismatch))?;
    if ctx.accounts.squadz_lobby.state != STATE_LAUNCHED {
        emit!(BurnSkipped { mint: SQUADZ_MINT, reason: 0 });
        return Ok(());
    }
    let Some(pool) = check_burn_pool(m, settings, &SQUADZ_MINT)? else {
        emit!(BurnSkipped { mint: SQUADZ_MINT, reason: 0 });
        return Ok(());
    };
    require_keys_eq!(*m[damm_swap_ix::INPUT].key, ctx.accounts.jar_wsol.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::OUTPUT].key, ctx.accounts.jar_coins.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[damm_swap_ix::PAYER].key, jar_key, LaunchError::AccountMismatch);

    let bump = ctx.bumps.burn_jar;
    let seeds: &[&[u8]] = &[BURN_JAR_SEED, &[bump]];
    let rent = Rent::get()?.minimum_balance(0);
    let sync = || token::sync_native(CpiContext::new(ctx.accounts.token_program.to_account_info(), SyncNative { account: ctx.accounts.jar_wsol.to_account_info() }));
    sync()?;
    let wrapped = meteora::token_balance(&ctx.accounts.jar_wsol)?;
    let spare = ctx.accounts.burn_jar.lamports().saturating_sub(rent + BURN_TIP);
    let amount = burn_size(&pool, wrapped.saturating_add(spare))?;
    let ideal = meteora::tokens_for_sol(amount, pool.sqrt_price).unwrap_or(0);
    if amount < BURN_MIN_AMOUNT || ideal == 0 {
        emit!(BurnSkipped { mint: SQUADZ_MINT, reason: 1 });
        return Ok(());
    }
    let to_wrap = amount.saturating_sub(wrapped);
    if to_wrap > 0 {
        system_transfer_signed(&ctx.accounts.system_program.to_account_info(), &ctx.accounts.burn_jar.to_account_info(), &ctx.accounts.jar_wsol.to_account_info(), to_wrap, &[seeds])?;
        sync()?;
    }
    let slippage = burn_slippage_bps(ctx.accounts.burn_state.last_burn_at, ctx.accounts.squadz_lobby.launched_at, now);
    let min_out = mul_div(ideal, BPS - slippage, BPS)?;
    let before = meteora::token_balance(&ctx.accounts.jar_coins)?;
    meteora::damm_swap(amount, min_out, m, &jar_key, &[seeds])?;
    let bought = meteora::token_balance(&ctx.accounts.jar_coins)?.saturating_sub(before);
    require!(bought >= min_out, LaunchError::TooFewCoins);
    // Destroys everything the jar holds, including any coins someone sent it.
    let burn = before.saturating_add(bought);
    token_2022::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_2022_program.to_account_info(),
            Burn { mint: ctx.accounts.mint.to_account_info(), from: ctx.accounts.jar_coins.to_account_info(), authority: ctx.accounts.burn_jar.to_account_info() },
            &[seeds],
        ),
        burn,
    )?;
    let tip = BURN_TIP.min(ctx.accounts.burn_jar.lamports().saturating_sub(rent));
    system_transfer_signed(&ctx.accounts.system_program.to_account_info(), &ctx.accounts.burn_jar.to_account_info(), &ctx.accounts.caller.to_account_info(), tip, &[seeds])?;

    let s = &mut ctx.accounts.burn_state;
    s.last_burn_at = now;
    s.sol_used = s.sol_used.saturating_add(amount);
    s.coins_burned = s.coins_burned.saturating_add(burn);
    emit!(Burned { mint: SQUADZ_MINT, sol: amount, coins: burn });
    Ok(())
}

#[event]
pub struct Burned {
    pub mint: Pubkey,
    pub sol: u64,
    pub coins: u64,
}

/// 0: the pool doesn't exist yet or is paused. 1: too little to burn. Either way the money waits, and the interval isn't used.
#[event]
pub struct BurnSkipped {
    pub mint: Pubkey,
    pub reason: u8,
}
