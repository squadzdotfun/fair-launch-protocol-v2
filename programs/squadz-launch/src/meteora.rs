//! Meteora calls. Callers pass Meteora's accounts through `remaining_accounts` in Meteora's order and check the
//! ones that decide ownership; Meteora checks the rest. Layouts: Meteora curve release 0.2.2, measured from real
//! settings and pools on devnet. Extra accounts at the end are passed along, so a small Meteora addition doesn't
//! stop anything, and our signature only ever goes to the one slot we've checked.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};

use crate::constants::*;
use crate::errors::LaunchError;

const INIT_HOOK: [u8; 8] = [182, 13, 233, 177, 42, 145, 135, 2];
const SWAP_HOOK: [u8; 8] = [183, 93, 153, 40, 24, 230, 194, 151];
const CONFIG_DISC: [u8; 8] = [40, 220, 194, 251, 41, 199, 123, 253];
const DAMM_POOL_DISC: [u8; 8] = [241, 154, 109, 4, 17, 177, 109, 188];
const DAMM_POSITION_DISC: [u8; 8] = [170, 188, 143, 228, 122, 64, 247, 208];
const DAMM_SETTINGS_DISC: [u8; 8] = [155, 12, 170, 224, 30, 250, 204, 130];
const DAMM_SETTINGS_CUSTOMIZABLE: u8 = 1;

/// `initialize_virtual_pool_with_token2022_transfer_hook`: 15 accounts.
pub const INIT_LEN: usize = 15;
pub mod init_ix {
    pub const CONFIG: usize = 0;
    pub const CREATOR: usize = 2;
    pub const BASE_MINT: usize = 3;
    pub const POOL: usize = 5;
    pub const PROGRAM: usize = 14;
}
/// `swap2_with_transfer_hook`: 15 accounts, then the hook's 3.
pub const SWAP_LEN: usize = 15;
pub const SWAP_WITH_HOOK_LEN: usize = SWAP_LEN + HOOK_ACCOUNTS;
pub const HOOK_ACCOUNTS: usize = 3;
pub mod swap_ix {
    pub const CONFIG: usize = 1;
    pub const POOL: usize = 2;
    pub const INPUT: usize = 3;
    pub const OUTPUT: usize = 4;
    pub const BASE_MINT: usize = 7;
    pub const PAYER: usize = 9;
    pub const REFERRAL: usize = 12;
    pub const PROGRAM: usize = 14;
}

pub const MODE_PARTIAL_FILL: u8 = 1;
pub const MODE_EXACT_OUT: u8 = 2;

/// Calls a Meteora program. Our signatures apply only to the slots listed.
fn invoke_slots(program: Pubkey, data: Vec<u8>, accounts: &[AccountInfo], slots: &[(usize, &Pubkey)], seeds: &[&[&[u8]]]) -> Result<()> {
    let metas = accounts
        .iter()
        .enumerate()
        .map(|(i, a)| AccountMeta { pubkey: *a.key, is_signer: a.is_signer || slots.iter().any(|(s, k)| *s == i && a.key == *k), is_writable: a.is_writable })
        .collect();
    invoke_signed(&Instruction { program_id: program, accounts: metas, data }, accounts, seeds)?;
    Ok(())
}

/// Calls a Meteora program. `signer_slot` is the one account our signature applies to.
fn invoke_slot(program: Pubkey, data: Vec<u8>, accounts: &[AccountInfo], signer_slot: usize, signer: &Pubkey, seeds: &[&[&[u8]]]) -> Result<()> {
    invoke_slots(program, data, accounts, &[(signer_slot, signer)], seeds)
}

/// The lobby signs as the coin's creator, and the contract signs for the coin's address, which only it can do.
pub fn create_pool(name: String, symbol: String, uri: String, accounts: &[AccountInfo], lobby: &Pubkey, mint: &Pubkey, seeds: &[&[&[u8]]]) -> Result<()> {
    let mut d = INIT_HOOK.to_vec();
    (name, symbol, uri).serialize(&mut d)?;
    invoke_slots(METEORA_DBC, d, accounts, &[(init_ix::CREATOR, lobby), (init_ix::BASE_MINT, mint)], seeds)
}

/// `amount_0` is SOL in (partial fill) or SOL out (exact out); `amount_1` is the least coins out or most coins in.
pub fn swap(amount_0: u64, amount_1: u64, mode: u8, accounts: &[AccountInfo], signer: &Pubkey, seeds: &[&[&[u8]]]) -> Result<()> {
    require!(accounts.len() >= SWAP_WITH_HOOK_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*accounts[swap_ix::PROGRAM].key, METEORA_DBC, LaunchError::AccountMismatch);
    let mut d = SWAP_HOOK.to_vec();
    (amount_0, amount_1, mode).serialize(&mut d)?;
    // TransferHookAccountsInfo: one slice, TransferHookBase, with the hook's accounts
    d.extend_from_slice(&1u32.to_le_bytes());
    d.push(0);
    d.push(HOOK_ACCOUNTS as u8);
    invoke_slot(METEORA_DBC, d, accounts, swap_ix::PAYER, signer, seeds)
}

/// `claim_creator_trading_fee2`: 13 accounts. Everything owed, with no hook accounts: fees are in SOL.
pub const CURVE_CLAIM_LEN: usize = 13;
pub mod curve_claim_ix {
    pub const POOL: usize = 1;
    pub const TOKEN_A: usize = 2;
    pub const TOKEN_B: usize = 3;
    pub const CREATOR: usize = 8;
    pub const PROGRAM: usize = 12;
}
pub fn claim_curve_fees(accounts: &[AccountInfo], signer: &Pubkey, seeds: &[&[&[u8]]]) -> Result<()> {
    require!(accounts.len() >= CURVE_CLAIM_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*accounts[curve_claim_ix::PROGRAM].key, METEORA_DBC, LaunchError::AccountMismatch);
    let mut d = vec![238, 247, 213, 94, 110, 145, 88, 142];
    (u64::MAX, u64::MAX).serialize(&mut d)?;
    d.extend_from_slice(&0u32.to_le_bytes());
    invoke_slot(METEORA_DBC, d, accounts, curve_claim_ix::CREATOR, signer, seeds)
}

/// DAMM v2 `claim_position_fee`: 15 accounts.
pub const DAMM_CLAIM_LEN: usize = 15;
pub mod damm_claim_ix {
    pub const POOL: usize = 1;
    pub const POSITION: usize = 2;
    pub const TOKEN_A: usize = 3;
    pub const TOKEN_B: usize = 4;
    pub const MINT_A: usize = 7;
    pub const OWNER: usize = 10;
    pub const PROGRAM: usize = 14;
}
pub fn claim_pool_fees(accounts: &[AccountInfo], signer: &Pubkey, seeds: &[&[&[u8]]]) -> Result<()> {
    require!(accounts.len() >= DAMM_CLAIM_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*accounts[damm_claim_ix::PROGRAM].key, METEORA_DAMM_V2, LaunchError::AccountMismatch);
    invoke_slot(METEORA_DAMM_V2, vec![180, 38, 154, 17, 133, 33, 162, 211], accounts, damm_claim_ix::OWNER, signer, seeds)
}

/// DAMM v2 `swap`: 14 accounts.
pub const DAMM_SWAP_LEN: usize = 14;
pub mod damm_swap_ix {
    pub const POOL: usize = 1;
    pub const INPUT: usize = 2;
    pub const OUTPUT: usize = 3;
    pub const VAULT_A: usize = 4;
    pub const VAULT_B: usize = 5;
    pub const MINT_A: usize = 6;
    pub const MINT_B: usize = 7;
    pub const PAYER: usize = 8;
    pub const REFERRAL: usize = 11;
    pub const PROGRAM: usize = 13;
}
pub fn damm_swap(amount_in: u64, min_out: u64, accounts: &[AccountInfo], signer: &Pubkey, seeds: &[&[&[u8]]]) -> Result<()> {
    require!(accounts.len() >= DAMM_SWAP_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*accounts[damm_swap_ix::PROGRAM].key, METEORA_DAMM_V2, LaunchError::AccountMismatch);
    let mut d = vec![248, 198, 158, 145, 225, 117, 135, 200];
    (amount_in, min_out).serialize(&mut d)?;
    invoke_slot(METEORA_DAMM_V2, d, accounts, damm_swap_ix::PAYER, signer, seeds)
}

/// The pool a coin moves to. Whoever moves a coin picks Meteora's settings account, so the contract applies Meteora's own
/// rule: a DAMM v2 settings account of the customizable type, private to Meteora's curve program. Only that program can
/// create a pool from one, and only when it moves that coin's curve, so no other pool for the coin can stand in, and a
/// settings account Meteora adds later still works. Offsets measured on devnet and mainnet, and they match Meteora's source.
/// The one account known today isn't read at all, so Meteora closing it can't stop fees and burns on coins already moved.
pub fn official_pool(mint: &Pubkey, settings: &AccountInfo) -> Result<Pubkey> {
    if *settings.key != METEORA_CUSTOM_SETTINGS {
        require_keys_eq!(*settings.owner, METEORA_DAMM_V2, LaunchError::AccountMismatch);
        let d = settings.try_borrow_data()?;
        require!(d.len() >= 328 && d[..8] == DAMM_SETTINGS_DISC && key_at(&d, 40) == METEORA_POOL_AUTHORITY && d[202] == DAMM_SETTINGS_CUSTOMIZABLE, LaunchError::AccountMismatch);
    }
    let (a, b) = if mint.to_bytes() > WSOL_MINT.to_bytes() { (*mint, WSOL_MINT) } else { (WSOL_MINT, *mint) };
    Ok(Pubkey::find_program_address(&[b"pool", settings.key.as_ref(), a.as_ref(), b.as_ref()], &METEORA_DAMM_V2).0)
}

pub struct DammPool {
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub sqrt_min_price: u128,
    pub sqrt_price: u128,
    pub activation_point: u64,
    /// false while Meteora has the pool paused.
    pub trading: bool,
    /// Liquidity locked for good. Nobody can add to it and take it back.
    pub locked_liquidity: u128,
}
pub fn read_damm_pool(info: &AccountInfo) -> Result<DammPool> {
    require_keys_eq!(*info.owner, METEORA_DAMM_V2, LaunchError::AccountMismatch);
    let d = info.try_borrow_data()?;
    require!(d.len() >= 568 && d[..8] == DAMM_POOL_DISC, LaunchError::AccountMismatch);
    Ok(DammPool {
        token_a_mint: key_at(&d, 168),
        token_b_mint: key_at(&d, 200),
        token_a_vault: key_at(&d, 232),
        token_b_vault: key_at(&d, 264),
        sqrt_min_price: u128::from_le_bytes(d[424..440].try_into().unwrap()),
        sqrt_price: u128::from_le_bytes(d[456..472].try_into().unwrap()),
        activation_point: u64_at(&d, 472),
        trading: d[481] == 0,
        locked_liquidity: u128::from_le_bytes(d[552..568].try_into().unwrap()),
    })
}

/// The SOL behind a pool's permanently locked liquidity, at the current price. Temporary liquidity can't raise it, and
/// unclaimed fees aren't in it. Meteora's formula is liquidity x (sqrt_price - sqrt_min_price) >> 128. Shifting in two
/// steps keeps it inside u128 and loses only dust. Too big to work out counts as zero, so a burn waits instead of running.
pub fn locked_sol(pool: &DammPool) -> u64 {
    let diff = pool.sqrt_price.saturating_sub(pool.sqrt_min_price);
    let sol = (pool.locked_liquidity >> 64).checked_mul(diff).map(|v| v >> 64).unwrap_or(0);
    u64::try_from(sol).unwrap_or(0)
}

/// True if this share of the pool holds most of its permanently locked liquidity. The coin's move locks all of its pool
/// share to the lobby, so a worthless share a stranger sends the lobby can't pass, and neither can a position elsewhere.
pub fn is_main_locked_position(position: &AccountInfo, pool: &AccountInfo) -> Result<bool> {
    require_keys_eq!(*position.owner, METEORA_DAMM_V2, LaunchError::AccountMismatch);
    let p = position.try_borrow_data()?;
    require!(p.len() >= 200 && p[..8] == DAMM_POSITION_DISC && key_at(&p, 8) == *pool.key, LaunchError::AccountMismatch);
    let d = pool.try_borrow_data()?;
    require!(d.len() >= 568, LaunchError::AccountMismatch);
    let locked = u128::from_le_bytes(p[184..200].try_into().unwrap());
    let pool_locked = u128::from_le_bytes(d[552..568].try_into().unwrap());
    Ok(pool_locked > 0 && locked > pool_locked / 2)
}

/// Coins `lamports` buy at a pool's current price, before fees: price is (sqrt_price / 2^64)^2 lamports per raw coin.
pub fn tokens_for_sol(lamports: u64, sqrt_price: u128) -> Option<u64> {
    if sqrt_price == 0 {
        return None;
    }
    let t = ((lamports as u128) << 64) / sqrt_price;
    if t >> 64 != 0 {
        return None;
    }
    u64::try_from((t << 64) / sqrt_price).ok()
}

/// Curve progress, from Meteora's pool account.
pub struct Progress {
    pub migrated: bool,
    pub leftover_withdrawn: bool,
}
pub fn read_progress(info: &AccountInfo) -> Result<Progress> {
    require_keys_eq!(*info.owner, METEORA_DBC, LaunchError::AccountMismatch);
    let d = info.try_borrow_data()?;
    require!(d.len() >= 320, LaunchError::AccountMismatch);
    Ok(Progress { migrated: d[308] == 3, leftover_withdrawn: d[309] == 1 })
}

fn u64_at(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}
fn key_at(d: &[u8], o: usize) -> Pubkey {
    Pubkey::new_from_array(d[o..o + 32].try_into().unwrap())
}

/// Curve sold amount and SOL threshold from a settings account.
pub struct Settings {
    pub swap_base_amount: u64,
    pub migration_quote_threshold: u64,
}
/// Reads a target's settings and refuses any setting that isn't the agreed one. A settings account can't change once
/// created, but if one were ever created wrong, the lobby must refuse rather than go ahead: refunds rely on the 0.25%
/// fee, and a wrong leftover receiver would lose the coins held outside the curve. Offsets measured on devnet.
pub fn read_settings(info: &AccountInfo, reserve: &Pubkey) -> Result<Settings> {
    require_keys_eq!(*info.owner, METEORA_DBC, LaunchError::AccountMismatch);
    let d = info.try_borrow_data()?;
    require!(d.len() >= 1080 && d[..8] == CONFIG_DISC, LaunchError::AccountMismatch);
    let zero = |a: usize, b: usize| d[a..b].iter().all(|x| *x == 0);
    let u16_at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    let expiry = u32::from_le_bytes(d[380..384].try_into().unwrap());
    // The opening fee's cutoff is 30 minutes. Test settings may use 120 seconds on a short-time build.
    let expiry_ok = expiry == 1_800 || (cfg!(feature = "short-time") && expiry == 120);
    require!(
        key_at(&d, 8) == WSOL_MINT                                  // priced in SOL
            && key_at(&d, 40) == *reserve                           // Meteora's fee share rights
            && key_at(&d, 72) == *reserve                           // the coins outside the curve come to us
            && u64_at(&d, 104) == CURVE_FEE_NUMERATOR as u64        // curve fee 0.25%...
            && u16_at(128) == 0 && d[130] == 0 && d[136] == 0       // ...flat: no fee schedule, no volatility fee
            && zero(184, 221)                                       // no liquidity vesting, no transfer fee
            && d[232] == 0                                          // curve fees in SOL
            && d[233] == 1                                          // moves to DAMM v2
            && d[234] == 1                                          // timed by clock, not slots
            && d[235] == COIN_DECIMALS
            && d[237] == 1                                          // Token-2022 coin
            && d[239] == 0 && d[240] == 0 && d[242] == 0            // no partner or unlocked creator liquidity...
            && d[241] == 100                                        // ...100% locked forever to the lobby
            && d[243] == 6                                          // our own pool fee
            && d[244] == 1                                          // fixed supply...
            && u64_at(&d, 344) == SUPPLY && u64_at(&d, 352) == SUPPLY
            && d[245] == 100                                        // curve's trading fee share all to the lobby
            && d[246] == 1                                          // coin metadata can't be changed, no mint power
            && d[247] == 0 && d[248] == 0                           // no move fee shares we couldn't claim
            && zero(296, 344)                                       // no locked vesting of supply
            && d[360] == 0 && d[361] == 0                           // pool fees in SOL, no volatility fee...
            && u16_at(362) == 800                                   // ...8% opening fee...
            && d[364] == 3                                          // ...on the market-cap schedule...
            && u16_at(366) == 0                                     // ...no compounding
            && u64_at(&d, 368) == 0                                 // no pool creation fee
            && d[376..380] == [100, 0, 73, 0]                       // the schedule: falling to 1.25% at 3x
            && d[384..392] == [184, 76, 10, 0, 0, 0, 0, 0]
            && expiry_ok
            && key_at(&d, 1048) == HOOK_PROGRAM,                    // our hook
        LaunchError::WrongMeteoraSettings
    );
    // What's left outside the curve and the pool has to cover the biggest treasury, or finalize could never run.
    let leftover = SUPPLY.saturating_sub(u64_at(&d, 256)).saturating_sub(u64_at(&d, 272));
    require!(leftover as u128 * BPS as u128 >= SUPPLY as u128 * MAX_TREASURY_BPS as u128, LaunchError::WrongMeteoraSettings);
    Ok(Settings { swap_base_amount: u64_at(&d, 256), migration_quote_threshold: u64_at(&d, 264) })
}

/// A Token-2022 coin with the full supply, no mint power and no freeze power.
pub fn mint_is_fixed(info: &AccountInfo) -> Result<bool> {
    require_keys_eq!(*info.owner, anchor_spl::token_2022::ID, LaunchError::AccountMismatch);
    let d = info.try_borrow_data()?;
    Ok(d.len() >= 82 && d[0..4] == [0; 4] && u64_at(&d, 36) == SUPPLY && d[46..50] == [0; 4])
}

/// SOL in the curve, owner checked.
pub fn quote_reserve(info: &AccountInfo) -> Result<u64> {
    require_keys_eq!(*info.owner, METEORA_DBC, LaunchError::AccountMismatch);
    let d = info.try_borrow_data()?;
    require!(d.len() >= 248, LaunchError::AccountMismatch);
    Ok(u64_at(&d, 240))
}

/// Token account balance (both token programs share this layout).
pub fn token_balance(info: &AccountInfo) -> Result<u64> {
    let d = info.try_borrow_data()?;
    require!(d.len() >= 72, LaunchError::AccountMismatch);
    Ok(u64_at(&d, 64))
}

/// A member's coin balance. A closed or foreign account counts as zero, so closing it can't dodge a holder check.
pub fn held_coins(info: &AccountInfo) -> u64 {
    if *info.owner != anchor_spl::token_2022::ID {
        return 0;
    }
    let Ok(d) = info.try_borrow_data() else { return 0 };
    if d.len() < 72 {
        return 0;
    }
    u64_at(&d, 64)
}
