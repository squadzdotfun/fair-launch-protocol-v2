pub mod after;
pub mod buy;
pub mod create;
pub mod refund;

pub use after::*;
pub use buy::*;
pub use create::*;
pub use refund::*;

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, program_pack::Pack};
use anchor_spl::token_2022::spl_token_2022;

use crate::constants::*;
use crate::errors::LaunchError;

pub const GATE_FLAG_OFFSET: usize = 8;

/// Opens or closes the coin's gate. The hook only lets coins move while it's open, and it never stays open past
/// one of our instructions.
pub fn set_gate(gate: &AccountInfo, open: bool) -> Result<()> {
    gate.try_borrow_mut_data()?[GATE_FLAG_OFFSET] = open as u8;
    Ok(())
}

/// Coin transfer that carries the hook's accounts. `hook_accounts`: gate, hook program, list.
#[allow(clippy::too_many_arguments)]
pub fn transfer_coins<'info>(
    token_program: &AccountInfo<'info>,
    from: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    authority: &AccountInfo<'info>,
    hook_accounts: &[AccountInfo<'info>],
    amount: u64,
    seeds: &[&[&[u8]]],
) -> Result<()> {
    spl_token_2022::onchain::invoke_transfer_checked(
        token_program.key,
        from.clone(),
        mint.clone(),
        to.clone(),
        authority.clone(),
        hook_accounts,
        amount,
        COIN_DECIMALS,
        seeds,
    )?;
    Ok(())
}

pub fn system_transfer_signed<'info>(system: &AccountInfo<'info>, from: &AccountInfo<'info>, to: &AccountInfo<'info>, lamports: u64, seeds: &[&[&[u8]]]) -> Result<()> {
    if lamports == 0 {
        return Ok(());
    }
    let ix = anchor_lang::solana_program::system_instruction::transfer(from.key, to.key, lamports);
    invoke_signed(&ix, &[from.clone(), to.clone(), system.clone()], seeds)?;
    Ok(())
}

/// Pays from a lobby's vault. Anyone can send a few lamports to the vault, and Solana refuses a payment that would
/// leave less than an empty account's rent but more than 0, so in that case the dust goes along with the payment.
pub fn pay_from_vault<'info>(system: &AccountInfo<'info>, vault: &AccountInfo<'info>, to: &AccountInfo<'info>, amount: u64, seeds: &[&[&[u8]]]) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let bal = vault.lamports();
    let rest = bal.saturating_sub(amount);
    let amount = if rest > 0 && rest < Rent::get()?.minimum_balance(0) { bal } else { amount };
    system_transfer_signed(system, vault, to, amount, seeds)
}

pub fn check_hook_accounts(hook_accounts: &[AccountInfo], gate: &Pubkey, mint: &Pubkey) -> Result<()> {
    require!(hook_accounts.len() == crate::meteora::HOOK_ACCOUNTS, LaunchError::AccountMismatch);
    require_keys_eq!(*hook_accounts[0].key, *gate, LaunchError::AccountMismatch);
    require_keys_eq!(*hook_accounts[1].key, HOOK_PROGRAM, LaunchError::AccountMismatch);
    let (list, _) = Pubkey::find_program_address(&[b"extra-account-metas", mint.as_ref()], &HOOK_PROGRAM);
    require_keys_eq!(*hook_accounts[2].key, list, LaunchError::AccountMismatch);
    Ok(())
}

// Keeps the Pack trait in scope for token account checks in the instruction files.
pub type WsolAccount = anchor_spl::token::spl_token::state::Account;
pub fn wsol_owner(info: &AccountInfo) -> Result<Pubkey> {
    let a = WsolAccount::unpack(&info.try_borrow_data()?).map_err(|_| error!(LaunchError::AccountMismatch))?;
    Ok(a.owner)
}
