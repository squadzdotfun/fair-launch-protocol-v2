//! Squadz Meteora program. One contract for every launch: the coin and its Meteora curve are public from the start,
//! only members can buy, everyone pays one flat price, refunds are self-serve, and the curve's completion launches it.
//! Built for devnet (--features devnet) or mainnet (--features mainnet); mainnet refuses to compile until its values
//! are set. Public source: github.com/squadzdotfun/fair-launch-protocol-v2.
use anchor_lang::prelude::*;
#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

// 9 Nov (founders, launch night): explorers show "Security.txt: true" from this; links point at the public v2 repo
#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Squadz Fair Launch Protocol v2",
    project_url: "https://squadz.fun",
    contacts: "email:security@squadz.fun",
    policy: "https://github.com/squadzdotfun/fair-launch-protocol-v2/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/squadzdotfun/fair-launch-protocol-v2",
    auditors: "None"
}

pub mod constants;
pub mod errors;
pub mod instructions;
pub mod meteora;
pub mod pass;
pub mod state;

use instructions::*;

#[cfg(feature = "devnet")]
declare_id!("GrW9TnN2gbbPoMmJtUniRGef9yDAfpqFeH95DLqP1bRA");
// Mainnet (founders, 9 Nov): keys\smp-mainnet-launch-program.json
#[cfg(feature = "mainnet")]
declare_id!("7g2QV5bdgV9LLdhFVJVzPN1bHW8RXk92WgKecQrNAjmw");

#[program]
pub mod squadz_launch {
    use super::*;

    pub fn create_lobby<'info>(ctx: Context<'_, '_, 'info, 'info, CreateLobby<'info>>, args: CreateArgs) -> Result<()> {
        instructions::create::handle(ctx, args)
    }

    pub fn buy<'info>(ctx: Context<'_, '_, 'info, 'info, Buy<'info>>, amount: u64, donation: u64, pass_expiry: i64) -> Result<()> {
        instructions::buy::handle(ctx, amount, donation, pass_expiry)
    }

    /// The creator's own buy in the transaction that creates the lobby (no join pass; creator, empty lobby, same moment only)
    pub fn creator_seat<'info>(ctx: Context<'_, '_, 'info, 'info, Buy<'info>>, amount: u64) -> Result<()> {
        instructions::buy::creator_seat(ctx, amount)
    }

    pub fn refund<'info>(ctx: Context<'_, '_, 'info, 'info, Refund<'info>>) -> Result<()> {
        instructions::refund::handle(ctx)
    }

    pub fn close_lobby(ctx: Context<CloseLobby>) -> Result<()> {
        instructions::refund::handle_close(ctx)
    }

    pub fn settle_launch(ctx: Context<SettleLaunch>) -> Result<()> {
        instructions::after::settle(ctx)
    }

    pub fn finalize_coins(ctx: Context<FinalizeCoins>) -> Result<()> {
        instructions::after::finalize(ctx)
    }

    pub fn claim_drops(ctx: Context<MemberClaim>) -> Result<()> {
        instructions::after::claim_drops(ctx)
    }

    pub fn spot_check<'info>(ctx: Context<'_, '_, 'info, 'info, SpotCheck<'info>>) -> Result<()> {
        instructions::after::spot_check(ctx)
    }

    pub fn release_treasury(ctx: Context<ReleaseTreasury>) -> Result<()> {
        instructions::after::release_treasury(ctx)
    }

    pub fn reclaim_treasury_fee(ctx: Context<ReclaimTreasuryFee>) -> Result<()> {
        instructions::after::reclaim_treasury_fee(ctx)
    }

    pub fn close_record(ctx: Context<CloseRecord>) -> Result<()> {
        instructions::after::close_record(ctx)
    }

    pub fn collect_curve_fees<'info>(ctx: Context<'_, '_, 'info, 'info, CollectFees<'info>>) -> Result<()> {
        instructions::after::collect_curve_fees(ctx)
    }

    pub fn collect_pool_fees<'info>(ctx: Context<'_, '_, 'info, 'info, CollectFees<'info>>) -> Result<()> {
        instructions::after::collect_pool_fees(ctx)
    }

    pub fn pay_creator_fees(ctx: Context<PayCreatorFees>) -> Result<()> {
        instructions::after::pay_creator_fees(ctx)
    }

    pub fn coin_buy_and_burn<'info>(ctx: Context<'_, '_, 'info, 'info, CoinBurn<'info>>) -> Result<()> {
        instructions::after::coin_buy_and_burn(ctx)
    }

    pub fn jar_buy_and_burn<'info>(ctx: Context<'_, '_, 'info, 'info, JarBurn<'info>>) -> Result<()> {
        instructions::after::jar_buy_and_burn(ctx)
    }

    pub fn burn_unearned_drops(ctx: Context<BurnUnearned>) -> Result<()> {
        instructions::after::burn_unearned_drops(ctx)
    }

    pub fn withdraw_ops(ctx: Context<WithdrawOps>) -> Result<()> {
        instructions::after::withdraw_ops(ctx)
    }
}
