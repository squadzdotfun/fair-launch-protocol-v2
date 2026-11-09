//! Creates a lobby: the coin, its Meteora curve, the gate and the hook's setup list, all in one transaction.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};
use anchor_lang::system_program;

use crate::constants::*;
use crate::errors::LaunchError;
use crate::meteora;
use crate::state::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateArgs {
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub gatekeeper: Pubkey,
    /// Receives the coin's fee share. All zeros means the creator's wallet.
    pub fee_wallet: Pubkey,
    /// Receives treasury coins. All zeros means the creator's wallet.
    pub treasury_wallet: Pubkey,
    /// Required ending of the coin's address. All zeros means none.
    pub ending: [u8; ENDING_LEN],
    /// The number the creator's browser found. Creator and number make the coin's address.
    pub coin_number: u64,
    pub mint_bump: u8,
    pub treasury_bps: u16,
    pub fee_choice: u8,
    pub approval_only: bool,
}

/// True if the base58 text of `key` ends with `suffix`. The last k base58 characters are the 32-byte number mod 58^k,
/// so this needs no string encoding.
fn ends_with_base58(key: &[u8; 32], suffix: &[u8]) -> bool {
    let mut m: u64 = 1;
    let mut target: u64 = 0;
    for c in suffix {
        let Some(d) = BASE58.iter().position(|x| x == c) else { return false };
        m *= 58;
        target = target * 58 + d as u64;
    }
    let mut r: u64 = 0;
    for b in key {
        r = (r * 256 + *b as u64) % m;
    }
    r == target
}

/// None, or 4 base58 characters that aren't another launchpad's ending.
fn ending_ok(e: &[u8; ENDING_LEN]) -> bool {
    if *e == [0u8; ENDING_LEN] {
        return true;
    }
    e.iter().all(|c| BASE58.contains(c)) && !RESERVED_ENDINGS.iter().any(|r| *r == e)
}

#[derive(Accounts)]
pub struct CreateLobby<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    /// CHECK: checked by seeds, receives the creation fee for upkeep
    #[account(mut, seeds = [OPS_SEED], bump)]
    pub ops: UncheckedAccount<'info>,
    /// CHECK: the coin's address, checked in the handler. Meteora creates the coin there, and only this contract can sign
    /// for it, so there's no secret key and nobody can create a coin there first.
    #[account(mut)]
    pub mint: UncheckedAccount<'info>,
    #[account(init, payer = creator, space = 8 + Lobby::INIT_SPACE, seeds = [LOBBY_SEED, mint.key().as_ref()], bump)]
    pub lobby: Box<Account<'info, Lobby>>,
    #[account(init, payer = creator, space = 8 + Gate::INIT_SPACE, seeds = [GATE_SEED, mint.key().as_ref()], bump)]
    pub gate: Box<Account<'info, Gate>>,
    /// CHECK: holds launch fees and donations until launch
    #[account(mut, seeds = [VAULT_SEED, mint.key().as_ref()], bump)]
    pub vault: UncheckedAccount<'info>,
    /// CHECK: checked by seeds, signs for the hook program
    #[account(seeds = [HOOK_ADMIN_SEED], bump)]
    pub hook_admin: UncheckedAccount<'info>,
    /// CHECK: created by the hook program, which checks it by seeds
    #[account(mut)]
    pub hook_list: UncheckedAccount<'info>,
    /// CHECK: address checked
    #[account(address = HOOK_PROGRAM)]
    pub hook_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: where donations go at launch. It must be writable here, so an address Solana always treats as read-only
    /// can't be chosen and later block settling.
    #[account(mut)]
    pub donation_wallet: UncheckedAccount<'info>,
}

pub fn handle<'info>(ctx: Context<'_, '_, 'info, 'info, CreateLobby<'info>>, args: CreateArgs) -> Result<()> {
    require!(args.name.len() <= MAX_NAME && args.symbol.len() <= MAX_SYMBOL && args.uri.len() <= MAX_URI, LaunchError::TooLong);
    require!(!args.name.is_empty() && !args.symbol.is_empty(), LaunchError::EmptyName);
    require!(args.treasury_bps <= MAX_TREASURY_BPS, LaunchError::BadTreasury);
    require!(args.treasury_bps % 100 == 0, LaunchError::TreasuryNotWhole);
    require!(args.gatekeeper != Pubkey::default(), LaunchError::PassKeyRequired);
    require!(args.fee_choice == FEES_TO_CREATOR || args.fee_choice == FEES_TO_COIN_BURN, LaunchError::BadFeeChoice);
    require!(ending_ok(&args.ending), LaunchError::BadEnding);

    // The coin's address comes from the creator and a number, the same method as the old contract. The search happens
    // off-chain, and the contract only checks the answer.
    let creator_key = ctx.accounts.creator.key();
    let number = args.coin_number.to_le_bytes();
    let bump = [args.mint_bump];
    let mint_seeds: &[&[u8]] = &[MINT_SEED, creator_key.as_ref(), &number, &bump];
    let expected_mint = Pubkey::create_program_address(mint_seeds, &crate::ID).map_err(|_| error!(LaunchError::AccountMismatch))?;
    require_keys_eq!(ctx.accounts.mint.key(), expected_mint, LaunchError::AccountMismatch);
    if args.ending != [0u8; ENDING_LEN] {
        require!(ends_with_base58(&expected_mint.to_bytes(), &args.ending), LaunchError::WrongEnding);
    }
    // A coin can't end in another launchpad's letters, whether or not it asked for an ending.
    require!(!RESERVED_ENDINGS.iter().any(|r| ends_with_base58(&expected_mint.to_bytes(), *r)), LaunchError::BadEnding);

    // Wallets can't be this lobby's own accounts, where money would be stuck for good.
    let fee_wallet = if args.fee_wallet == Pubkey::default() { creator_key } else { args.fee_wallet };
    let treasury_wallet = if args.treasury_wallet == Pubkey::default() { creator_key } else { args.treasury_wallet };
    let donation_info = ctx.accounts.donation_wallet.to_account_info();
    let own = [ctx.accounts.lobby.key(), ctx.accounts.vault.key(), ctx.accounts.gate.key(), ctx.accounts.ops.key()];
    require!(!donation_info.executable && !own.contains(donation_info.key) && !own.contains(&fee_wallet) && !own.contains(&treasury_wallet), LaunchError::BadWallet);

    let m = ctx.remaining_accounts;
    require!(m.len() >= meteora::INIT_LEN, LaunchError::AccountMismatch);
    require_keys_eq!(*m[meteora::init_ix::PROGRAM].key, METEORA_DBC, LaunchError::AccountMismatch);
    let config = *m[meteora::init_ix::CONFIG].key;
    let target = target_for(&config).ok_or(LaunchError::BadTarget)?;
    require_keys_eq!(*m[meteora::init_ix::CREATOR].key, ctx.accounts.lobby.key(), LaunchError::AccountMismatch);
    require_keys_eq!(*m[meteora::init_ix::BASE_MINT].key, ctx.accounts.mint.key(), LaunchError::AccountMismatch);
    let reserve = Pubkey::find_program_address(&[RESERVE_SEED], &crate::ID).0;
    let settings = meteora::read_settings(&m[meteora::init_ix::CONFIG], &reserve)?;
    // The threshold has to be what members' money really reaches: the target, less the launch fee and Meteora's fee.
    // Otherwise the mismatch only shows when the lobby is nearly full. The tolerance covers rounding when settings are made.
    let reaches_curve = (target as u128) * ((BPS - LAUNCH_FEE_BPS) as u128) / (BPS as u128) * (FEE_DENOMINATOR - CURVE_FEE_NUMERATOR) / FEE_DENOMINATOR;
    let threshold = settings.migration_quote_threshold as u128;
    require!(threshold <= reaches_curve && reaches_curve - threshold <= 2_000, LaunchError::WrongMeteoraSettings);

    let mint_key = ctx.accounts.mint.key();
    let lobby_seeds: &[&[u8]] = &[LOBBY_SEED, mint_key.as_ref(), &[ctx.bumps.lobby]];
    let lobby_key = ctx.accounts.lobby.key();
    meteora::create_pool(args.name, args.symbol, args.uri, m, &lobby_key, &mint_key, &[lobby_seeds, mint_seeds])?;
    require!(meteora::mint_is_fixed(&m[meteora::init_ix::BASE_MINT])?, LaunchError::CoinNotFixed);

    // The hook's list: only our hook_admin can create it.
    let admin_seeds: &[&[u8]] = &[HOOK_ADMIN_SEED, &[ctx.bumps.hook_admin]];
    let ix = Instruction {
        program_id: HOOK_PROGRAM,
        accounts: vec![
            AccountMeta::new(ctx.accounts.creator.key(), true),
            AccountMeta::new(ctx.accounts.hook_list.key(), false),
            AccountMeta::new_readonly(mint_key, false),
            AccountMeta::new_readonly(ctx.accounts.hook_admin.key(), true),
            AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
        ],
        data: vec![92, 197, 174, 197, 41, 124, 19, 3],
    };
    invoke_signed(
        &ix,
        &[
            ctx.accounts.creator.to_account_info(),
            ctx.accounts.hook_list.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.hook_admin.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.hook_program.to_account_info(),
        ],
        &[admin_seeds],
    )?;

    // Members' coins for the whole target: what the curve sells, less a rounding buffer, less half of any treasury.
    let members_bps = MEMBERS_BPS - (args.treasury_bps as u64) / 2;
    let coins_for_target = (settings.swap_base_amount as u128)
        .checked_mul((BPS - FLAT_RATE_BUFFER_BPS) as u128)
        .and_then(|v| v.checked_mul(members_bps as u128))
        .map(|v| v / (BPS as u128) / (MEMBERS_BPS as u128))
        .ok_or(LaunchError::Overflow)? as u64;

    // Creation fee, approval fee and treasury fee. The team's own launch pays no treasury fee.
    let sys = ctx.accounts.system_program.to_account_info();
    let creator_info = ctx.accounts.creator.to_account_info();
    let upkeep = CREATION_FEE + if args.approval_only { APPROVAL_FEE } else { 0 };
    system_program::transfer(CpiContext::new(sys.clone(), system_program::Transfer { from: creator_info.clone(), to: ctx.accounts.ops.to_account_info() }), upkeep)?;
    let treasury_fee = if ctx.accounts.creator.key() == SQUADZ_CREATOR { 0 } else { (args.treasury_bps as u64) * TREASURY_FEE_PER_BPS };
    if treasury_fee > 0 {
        system_program::transfer(CpiContext::new(sys, system_program::Transfer { from: creator_info, to: ctx.accounts.vault.to_account_info() }), treasury_fee)?;
    }

    let now = Clock::get()?.unix_timestamp;
    let l = &mut ctx.accounts.lobby;
    l.treasury_fee_held = treasury_fee;
    l.creator = ctx.accounts.creator.key();
    l.mint = mint_key;
    l.pool = *m[meteora::init_ix::POOL].key;
    l.config = config;
    l.gatekeeper = args.gatekeeper;
    l.donation_wallet = ctx.accounts.donation_wallet.key();
    l.fee_wallet = fee_wallet;
    l.treasury_wallet = treasury_wallet;
    l.ending = args.ending;
    l.target = target;
    l.threshold = settings.migration_quote_threshold;
    l.coins_for_target = coins_for_target;
    l.created_at = now;
    l.last_buy_at = now;
    l.treasury_bps = args.treasury_bps;
    l.state = STATE_OPEN;
    l.fee_choice = args.fee_choice;
    l.approval_only = args.approval_only;
    l.bump = ctx.bumps.lobby;
    l.gate_bump = ctx.bumps.gate;
    l.vault_bump = ctx.bumps.vault;
    ctx.accounts.gate.open = 0;

    emit!(LobbyCreated { lobby: lobby_key, mint: mint_key, target, coins_for_target });
    Ok(())
}

#[event]
pub struct LobbyCreated {
    pub lobby: Pubkey,
    pub mint: Pubkey,
    pub target: u64,
    pub coins_for_target: u64,
}
