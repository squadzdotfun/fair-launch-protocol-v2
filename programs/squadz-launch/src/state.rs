use anchor_lang::prelude::*;

pub const STATE_OPEN: u8 = 0;
pub const STATE_CLOSED: u8 = 1;
pub const STATE_LAUNCHED: u8 = 2;

pub const FEES_TO_CREATOR: u8 = 0;
pub const FEES_TO_COIN_BURN: u8 = 1;

#[account]
#[derive(InitSpace)]
pub struct Lobby {
    pub creator: Pubkey,
    pub mint: Pubkey,
    pub pool: Pubkey,
    pub config: Pubkey,
    pub gatekeeper: Pubkey,
    /// Where members' donations go at launch.
    pub donation_wallet: Pubkey,
    pub target: u64,
    pub threshold: u64,
    /// Coins per target's worth of SOL, so coins = paid * coins_for_target / target.
    pub coins_for_target: u64,
    /// SOL members have in right now.
    pub total_paid: u64,
    pub launch_fee_held: u64,
    pub donations_total: u64,
    pub created_at: i64,
    pub last_buy_at: i64,
    pub launched_at: i64,
    pub members: u32,
    pub treasury_bps: u16,
    pub state: u8,
    pub fee_choice: u8,
    pub approval_only: bool,
    pub bump: u8,
    pub gate_bump: u8,
    pub vault_bump: u8,
    // --- after launch ---
    pub treasury_fee_held: u64,
    pub treasury_total: u64,
    pub treasury_released: u64,
    pub hodl_pot: u64,
    /// SOL put in by members still above the hold line. Sizes each drop's shares.
    pub qualified_paid: u64,
    /// How much SOL each released drop was shared across.
    pub qualified_at: [u64; 5],
    /// Drops released so far. The next one releases once every member still in has been checked since it came due.
    pub drops_done: u8,
    /// Members still in who have been checked for the drop that's due now.
    pub round_checked: u32,
    pub settled: bool,
    pub coins_finalized: bool,
    /// False until the last collection split 50/50: the real share's first after the opening window. After
    /// OPENING_SPLIT_LIMIT, anything that isn't the real share splits 10/90 without changing this.
    pub opening_done: bool,
    pub pool_opened_at: i64,
    /// SOL set aside for this coin's own buy and burn.
    pub coin_burn_sol: u64,
    pub members_out: u32,
    /// Receives the coin's fee share. Chosen at creation, the creator's wallet by default.
    pub fee_wallet: Pubkey,
    /// Receives treasury coins. Chosen at creation, the creator's wallet by default.
    pub treasury_wallet: Pubkey,
    /// Required ending of the coin's address. All zeros means none.
    pub ending: [u8; 4],
    /// Last time this coin's own buy and burn ran.
    pub last_burn_at: i64,
    /// Drops nobody earned have been burned. Happens once at most.
    pub unearned_burned: bool,
    /// Spare space, all zeros, so an upgrade can add fields without making existing lobbies unreadable.
    pub reserved: [u8; 128],
}

impl Lobby {
    /// Closed by the creator, or no buys for 30 days, which also means it can never launch.
    pub fn is_over(&self, now: i64) -> bool {
        self.state == STATE_CLOSED || (self.state == STATE_OPEN && now >= self.last_buy_at + crate::constants::INACTIVITY_LIMIT)
    }
}

/// Timer and totals for the $SQUADZ burn jar.
#[account]
#[derive(InitSpace)]
pub struct BurnState {
    pub last_burn_at: i64,
    pub sol_used: u64,
    pub coins_burned: u64,
}

/// One member's buy. The deposit returns when it closes.
#[account]
#[derive(InitSpace)]
pub struct Member {
    pub lobby: Pubkey,
    pub wallet: Pubkey,
    pub paid: u64,
    pub swap_in: u64,
    pub fee_held: u64,
    pub donation: u64,
    /// Their coins, recorded at the buy. The 75% hold line is measured against this.
    pub coins: u64,
    /// Fell below the hold line. Permanent.
    pub out: bool,
    pub next_drop: u8,
    pub bump: u8,
    /// How many drops had released when this member was marked out. They keep those, and no later ones.
    pub out_at: u8,
    /// How many drops this member has been checked for.
    pub checked_for: u8,
    /// Spare space, all zeros, so an upgrade can add fields without making existing records unreadable.
    pub reserved: [u8; 32],
}

/// The coin's gate. The hook reads its flag; only this contract ever writes it.
#[account]
#[derive(InitSpace)]
pub struct Gate {
    pub open: u8,
}
