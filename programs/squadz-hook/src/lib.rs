//! Transfer hook for the Squadz Meteora program. A coin can only move while our launch contract
//! is running one of its own instructions, which it shows by opening the coin's gate. So outsiders
//! can't buy, members can't sell or send, and nobody can route curve coins anywhere else.
//! Meteora removes this hook in the swap that completes the curve, and the coin is then free.
use anchor_lang::prelude::*;
use anchor_lang::system_program::{allocate, assign, transfer, Allocate, Assign, Transfer};
use spl_tlv_account_resolution::{account::ExtraAccountMeta, state::ExtraAccountMetaList};
use spl_transfer_hook_interface::instruction::{ExecuteInstruction, TransferHookInstruction};
#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

// 9 Nov (founders, launch night): explorers show "Security.txt: true" from this; links point at the public v2 repo
#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Squadz Fair Launch Protocol v2 (transfer hook)",
    project_url: "https://squadz.fun",
    contacts: "email:security@squadz.fun",
    policy: "https://github.com/squadzdotfun/fair-launch-protocol-v2/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/squadzdotfun/fair-launch-protocol-v2",
    auditors: "None"
}

// Every build picks devnet or mainnet, so a test id can't reach a mainnet build.
#[cfg(not(any(feature = "devnet", feature = "mainnet")))]
compile_error!("Choose a build: --features devnet, or mainnet.");
#[cfg(all(feature = "devnet", feature = "mainnet"))]
compile_error!("A build is devnet or mainnet, never both.");

#[cfg(feature = "devnet")]
declare_id!("MF1FtHVMcxjxVRLCno7pzbgPpfGZZ7fRuNqMp838Lrb");
#[cfg(feature = "devnet")]
pub const LAUNCH_PROGRAM: Pubkey = pubkey!("GrW9TnN2gbbPoMmJtUniRGef9yDAfpqFeH95DLqP1bRA");

// Mainnet (founders, 9 Nov): keys\smp-mainnet-hook-program.json and keys\smp-mainnet-launch-program.json
#[cfg(feature = "mainnet")]
declare_id!("J78vo6a5GaE58zxwhy8tw3S8GvMSx5WeuUzBfD4NnN4j");
#[cfg(feature = "mainnet")]
pub const LAUNCH_PROGRAM: Pubkey = pubkey!("7g2QV5bdgV9LLdhFVJVzPN1bHW8RXk92WgKecQrNAjmw");
/// The gate's flag: 8 bytes of Anchor discriminator, then 1 = open.
pub const GATE_FLAG_OFFSET: usize = 8;

#[program]
pub mod squadz_hook {
    use super::*;

    /// Only our launch contract can create a coin's list, so nobody can set one up wrong first and freeze the coin.
    pub fn initialize_extra_account_meta_list(ctx: Context<InitializeExtraAccountMetaList>) -> Result<()> {
        let mint = ctx.accounts.mint.key();
        let (gate, _) = Pubkey::find_program_address(&[b"gate", mint.as_ref()], &LAUNCH_PROGRAM);
        let metas = vec![ExtraAccountMeta::new_with_pubkey(&gate, false, false).map_err(|_| error!(HookError::BadMetaList))?];
        let size = ExtraAccountMetaList::size_of(metas.len()).map_err(|_| error!(HookError::BadMetaList))? as u64;
        let lamports = Rent::get()?.minimum_balance(size as usize);
        let seeds: &[&[u8]] = &[b"extra-account-metas", mint.as_ref(), &[ctx.bumps.extra_account_meta_list]];
        let sys = ctx.accounts.system_program.to_account_info();
        let list = ctx.accounts.extra_account_meta_list.to_account_info();
        // Someone may have sent SOL here first to block the coin, which plain account creation refuses. So top up,
        // then claim the space and ownership, which works either way.
        let top_up = lamports.saturating_sub(list.lamports());
        if top_up > 0 {
            transfer(CpiContext::new(sys.clone(), Transfer { from: ctx.accounts.payer.to_account_info(), to: list.clone() }), top_up)?;
        }
        allocate(CpiContext::new_with_signer(sys.clone(), Allocate { account_to_allocate: list.clone() }, &[seeds]), size)?;
        assign(CpiContext::new_with_signer(sys, Assign { account_to_assign: list }, &[seeds]), ctx.program_id)?;
        ExtraAccountMetaList::init::<ExecuteInstruction>(&mut ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?, &metas)
            .map_err(|_| error!(HookError::BadMetaList))?;
        Ok(())
    }

    pub fn fallback<'info>(_program_id: &Pubkey, accounts: &'info [AccountInfo<'info>], data: &[u8]) -> Result<()> {
        match TransferHookInstruction::unpack(data).map_err(|_| error!(HookError::UnknownInstruction))? {
            TransferHookInstruction::Execute { .. } => check_gate(accounts),
            _ => err!(HookError::UnknownInstruction),
        }
    }
}

/// Accounts: 0 source, 1 mint, 2 destination, 3 owner, 4 list, 5 gate.
fn check_gate(accounts: &[AccountInfo]) -> Result<()> {
    require!(accounts.len() >= 6, HookError::MissingGate);
    let (mint, gate) = (&accounts[1], &accounts[5]);
    let (expected, _) = Pubkey::find_program_address(&[b"gate", mint.key.as_ref()], &LAUNCH_PROGRAM);
    require_keys_eq!(*gate.key, expected, HookError::MissingGate);
    require_keys_eq!(*gate.owner, LAUNCH_PROGRAM, HookError::MissingGate);
    let d = gate.try_borrow_data()?;
    require!(d.len() > GATE_FLAG_OFFSET && d[GATE_FLAG_OFFSET] == 1, HookError::MembersOnly);
    Ok(())
}

#[derive(Accounts)]
pub struct InitializeExtraAccountMetaList<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: created here, checked by seeds
    #[account(mut, seeds = [b"extra-account-metas", mint.key().as_ref()], bump)]
    pub extra_account_meta_list: UncheckedAccount<'info>,
    /// CHECK: the coin this list belongs to
    pub mint: UncheckedAccount<'info>,
    /// Signs only inside our launch contract
    #[account(seeds = [b"hook_admin"], bump, seeds::program = LAUNCH_PROGRAM)]
    pub hook_admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum HookError {
    #[msg("Could not build the extra account list")]
    BadMetaList,
    #[msg("Unknown hook instruction")]
    UnknownInstruction,
    #[msg("Coin gate account missing or wrong")]
    MissingGate,
    #[msg("Members only until launch: join at squadz.fun")]
    MembersOnly,
}
