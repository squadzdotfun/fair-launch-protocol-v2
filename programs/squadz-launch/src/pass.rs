//! Join passes, same as the old contract. A pass is an ed25519 signature over `pass_message`, checked by Solana's
//! Ed25519 program in the instruction directly before the buy. It's tied to one lobby, one wallet and an expiry time.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::ed25519_program;
use anchor_lang::solana_program::sysvar::instructions::{load_current_index_checked, load_instruction_at_checked};

use crate::errors::LaunchError;

const PASS_TAG: &[u8] = b"SQUADZ_METEORA_PASS";

pub fn pass_message(lobby: &Pubkey, wallet: &Pubkey, expiry: i64) -> Vec<u8> {
    let mut m = Vec::with_capacity(PASS_TAG.len() + 3 * 32 + 8);
    m.extend_from_slice(PASS_TAG);
    m.extend_from_slice(crate::ID.as_ref());
    m.extend_from_slice(lobby.as_ref());
    m.extend_from_slice(wallet.as_ref());
    m.extend_from_slice(&expiry.to_le_bytes());
    m
}

const HEADER: usize = 2;
const OFFSETS: usize = 14;
const SELF: u16 = u16::MAX;

pub fn verify_pass(ix_sysvar: &AccountInfo, gatekeeper: &Pubkey, lobby: &Pubkey, wallet: &Pubkey, expiry: i64) -> Result<()> {
    let current = load_current_index_checked(ix_sysvar)? as usize;
    require!(current > 0, LaunchError::PassMissing);
    let ix = load_instruction_at_checked(current - 1, ix_sysvar)?;
    require_keys_eq!(ix.program_id, ed25519_program::ID, LaunchError::PassMissing);
    let d = &ix.data;
    require!(d.len() >= HEADER + OFFSETS && d[0] == 1, LaunchError::PassMissing);
    let at = |i: usize| u16::from_le_bytes([d[HEADER + i], d[HEADER + i + 1]]);
    let (sig_off, sig_ix) = (at(0) as usize, at(2));
    let (key_off, key_ix) = (at(4) as usize, at(6));
    let (msg_off, msg_len, msg_ix) = (at(8) as usize, at(10) as usize, at(12));
    // Everything must live inside the verify instruction itself, so nobody can point the check at other bytes.
    require!(sig_ix == SELF && key_ix == SELF && msg_ix == SELF, LaunchError::PassMissing);
    require!(d.len() >= sig_off + 64 && d.len() >= key_off + 32 && d.len() >= msg_off + msg_len, LaunchError::PassMissing);
    require!(&d[key_off..key_off + 32] == gatekeeper.as_ref(), LaunchError::BadPass);
    require!(d[msg_off..msg_off + msg_len] == pass_message(lobby, wallet, expiry)[..], LaunchError::BadPass);
    Ok(())
}
