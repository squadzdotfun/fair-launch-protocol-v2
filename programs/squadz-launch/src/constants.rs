use anchor_lang::prelude::*;

// Every build picks devnet or mainnet, and each has its own values below, so a test value can't reach a mainnet build.
#[cfg(not(any(feature = "devnet", feature = "mainnet")))]
compile_error!("Choose a build: --features devnet, or mainnet.");
#[cfg(all(feature = "devnet", feature = "mainnet"))]
compile_error!("A build is devnet or mainnet, never both.");
#[cfg(all(feature = "short-time", not(feature = "devnet")))]
compile_error!("short-time is for devnet tests only.");

/// Values that differ between devnet and mainnet. Only one of these is compiled.
#[cfg(feature = "devnet")]
mod network {
    use super::*;
    pub const HOOK_PROGRAM: Pubkey = pubkey!("MF1FtHVMcxjxVRLCno7pzbgPpfGZZ7fRuNqMp838Lrb");
    /// Where creation and approval fees can be withdrawn to.
    pub const OPS_WALLET: Pubkey = pubkey!("7xcUXuZMtvYPfQM9PXNEUP2qS7SjoctjyVwVKZwfN9Wa");
    /// The team's wallet pays no treasury fee.
    pub const SQUADZ_CREATOR: Pubkey = pubkey!("7xcUXuZMtvYPfQM9PXNEUP2qS7SjoctjyVwVKZwfN9Wa");
    /// $SQUADZ's coin, which the burn jar buys and burns. Test coin number 82375533, see tests/logs/squadz-coin.json. The
    /// earlier test coin's lobby has the old account layout, so it can't be read after the spare space was added.
    pub const SQUADZ_MINT: Pubkey = pubkey!("BeCxriijzXWKfNrXvgEQHM4AGQE9DaqYBkishfZiFAiR");
    /// Allowed targets and their Meteora settings, one per target. The 4 and 5 SOL targets are for tests only.
    pub const TARGETS: &[(u64, Pubkey)] = &[
        (75 * SOL, pubkey!("7oSg99iSViGgVgCp1E3xcKVbDZU4MWjExDyhE1LcY5K3")),
        (100 * SOL, pubkey!("F8ji4nY1qQEkVnnaLDWxEqi98WMKehqs16B19xZ1CKJH")),
        (125 * SOL, pubkey!("34GQDYa85nJqZET2KUNmT2UP7Wvv7oAn7maJhMHkedYz")),
        (150 * SOL, pubkey!("4iM4Hp25N1bnfAKvvrtRcH7bpeNZBKSmhbYFky5N3Dg6")),
        (4 * SOL, pubkey!("3uQ3UFwwUYFzH9maKFWWTwEDGrLpDVhFsF5JtZ9WLnTy")),
        // Same as the 4 SOL settings, with a 120-second opening fee, so the cutoff can be tested quickly.
        (5 * SOL, pubkey!("EEPKTabA9ikEuMnfiyCGRs2D1xXqSR8Fxt6kpbqzu4ij")),
    ];
}
/// Mainnet values (founders, 9 Nov, launch night).
#[cfg(feature = "mainnet")]
mod network {
    use super::*;
    pub const HOOK_PROGRAM: Pubkey = pubkey!("J78vo6a5GaE58zxwhy8tw3S8GvMSx5WeuUzBfD4NnN4j");
    /// Where creation and approval fees can be withdrawn to: the mainnet bot wallet, so the bots fund themselves.
    pub const OPS_WALLET: Pubkey = pubkey!("4SRVBrC6umt9TaA4cN3n6TtPQY5s96sHcf1M8Xcp8jSK");
    /// The team's launch pays no treasury fee: vault 0 of the team multisig (Squads v4 3AY6F8wNDrQP4FBmKHffrXu63hFkCFsYnDaSUv1TpDGz).
    pub const SQUADZ_CREATOR: Pubkey = pubkey!("4pki8NpwGpGZf3SUF7XZ12u2xiZfhdfX4vd8KQNbMHRC");
    /// $SQUADZ's coin, which the burn jar buys and burns: creator vault 0, coin number 11223978, bump 255 (scripts/mainnet/find-squadz-coin.js).
    pub const SQUADZ_MINT: Pubkey = pubkey!("BnYu2TaX7dSitcsgN1KCnmwQY542EpodHBKgeLRNFAiR");
    /// Only 75, 100, 125 and 150 SOL. No test targets. Settings made by tests/make-config-mainnet.js.
    pub const TARGETS: &[(u64, Pubkey)] = &[
        (75 * SOL, pubkey!("64YtqLayh5bpwvs6qNUd76DN42ppBjYZMqPDmnxnxuyE")),
        (100 * SOL, pubkey!("EGB1zx1z2pxwap2vDreVP8zr1rNrYRyvsoZZLC3jynr8")),
        (125 * SOL, pubkey!("HwxBqEnf1EaoejQX25KSRztMQdmMwUNPQ3Dw7EzuM4oe")),
        (150 * SOL, pubkey!("J2sTuy2Ec6xyPamCXTMzkAYwq6H45daTJomX9XNv4EjJ")),
    ];
}
pub use network::*;

pub const METEORA_DBC: Pubkey = pubkey!("dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN");
pub const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");

pub const MINT_SEED: &[u8] = b"mint";
pub const LOBBY_SEED: &[u8] = b"lobby";
pub const GATE_SEED: &[u8] = b"gate";
pub const VAULT_SEED: &[u8] = b"vault";
pub const MEMBER_SEED: &[u8] = b"member";
pub const RESERVE_SEED: &[u8] = b"reserve";
pub const BURN_JAR_SEED: &[u8] = b"burn_jar";
pub const HOOK_ADMIN_SEED: &[u8] = b"hook_admin";

pub const SOL: u64 = 1_000_000_000;
pub const COIN_DECIMALS: u8 = 6;
pub const BPS: u64 = 10_000;

/// The four buy amounts.
pub const BUY_AMOUNTS: [u64; 4] = [SOL / 2, SOL, SOL * 3 / 2, SOL * 2];
/// One wallet can buy more than once (top-ups, founders 8 Nov), up to this much in total.
pub const MAX_PER_WALLET: u64 = SOL * 2;
pub const LAUNCH_FEE_BPS: u64 = 500;
pub const REFUND_FEE: u64 = SOL / 100;
pub const MIN_DONATION: u64 = SOL / 1000;
/// Meteora's curve fee, 0.25%, out of 1_000_000_000.
pub const CURVE_FEE_NUMERATOR: u128 = 2_500_000;
pub const FEE_DENOMINATOR: u128 = 1_000_000_000;
/// Meteora rounds its curve maths against the seller, so the last refund asks for a touch less than the curve holds.
pub const RESERVE_ROUNDING_MARGIN: u128 = 1_000;
/// Added to the final buy so rounding can't leave the curve just short. Meteora takes only what it needs.
pub const LAST_BUY_MARGIN: u64 = 1_000;
/// Members' share of the coins the curve sells. Kept under 100% as a rounding buffer.
pub const FLAT_RATE_BUFFER_BPS: u64 = 10;
/// Members' base share of supply, in bps. A treasury takes half of its size from this.
pub const MEMBERS_BPS: u64 = 5_000;
pub const MAX_TREASURY_BPS: u16 = 2_000;

#[cfg(not(feature = "short-time"))]
pub const INACTIVITY_LIMIT: i64 = 30 * 86_400;
#[cfg(feature = "short-time")]
/// Devnet test build: 24 hours (founders, 7 Nov; was 120 s), so a test lobby stays open long enough to look at by hand.
pub const INACTIVITY_LIMIT: i64 = 86_400;

pub const METEORA_DAMM_V2: Pubkey = pubkey!("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG");
pub const SUPPLY: u64 = 1_000_000_000_000_000;

pub const OPS_SEED: &[u8] = b"ops";
/// What creating a lobby costs its creator in all (founders, 10 Nov): the rent of every account creating makes, plus
/// the upkeep fee. The contract works out the rent and takes only the rest as the fee, so the total is always this
/// amount, whatever the coin's name, ticker and link sizes are. Not included: Solana's network fee, the treasury fee,
/// and the creator's own buy.
pub const CREATION_TOTAL: u64 = SOL / 20;
/// On top of the total, for an approval-only lobby: the two together are a flat 0.1 SOL.
pub const APPROVAL_FEE: u64 = SOL / 20;
/// Sizes of the lobby's two token accounts, which the creator opens right after `create_lobby`, in the same
/// transaction: its coin account (Token-2022: 165 + account type + hook marker + fixed-owner marker) and its
/// wrapped-SOL account. Their rent counts towards the total.
pub const COIN_VAULT_LEN: usize = 175;
pub const WSOL_VAULT_LEN: usize = 165;
pub const TREASURY_FEE_PER_BPS: u64 = SOL / 100;
/// Kept from the launch fee for the upkeep account, covering the move to the pool (measured 0.0165 SOL). Meteora
/// doesn't record who moved a coin, so this can't pay the mover directly without letting strangers take it.
pub const MOVE_TO_POOL_COST: u64 = SOL / 50;

/// Pool fees after Meteora's cut: 50/50 in the opening window, then 10% to the $SQUADZ burn.
pub const OPENING_BURN_BPS: u64 = 5_000;
pub const NORMAL_BURN_BPS: u64 = 1_000;
pub const HOLD_LINE_BPS: u64 = 7_500;
pub const DROP_BPS: [u64; 5] = [1_000, 1_500, 2_000, 2_500, 3_000];
pub const TREASURY_RELEASE_BPS: [u64; 4] = [1_000, 3_000, 3_000, 3_000];

// Buy and burn limits, the same as the old contract. Anyone can trigger a burn, so size and timing are fixed here.
pub const BURN_STATE_SEED: &[u8] = b"burn_state";
/// A burn spends at most this share of the SOL behind the pool's permanently locked liquidity.
pub const BURN_MAX_POOL_BPS: u64 = 50;
/// Smaller burns are skipped, without using up the interval.
pub const BURN_MIN_AMOUNT: u64 = SOL / 1000;
/// Normal price limit: 1.25% fee plus price impact.
pub const BURN_MAX_SLIPPAGE_BPS: u64 = 200;
/// Price limit once no burn has succeeded for BURN_STUCK_AFTER, so a burn can't get stuck for good.
pub const BURN_STUCK_SLIPPAGE_BPS: u64 = 1_100;
/// Paid from the jar to whoever runs a jar burn.
pub const BURN_TIP: u64 = 50_000;
/// A coin's own buy and burn runs every 15 minutes (founders, 10 Nov; hourly before), the same as the jar below.
#[cfg(not(feature = "short-time"))]
pub const BURN_INTERVAL: i64 = 900;
#[cfg(feature = "short-time")]
pub const BURN_INTERVAL: i64 = 30;
/// The $SQUADZ burn jar burns every 15 minutes (founders, 8 Nov: so a busy jar keeps up). Each burn keeps the same
/// 0.5% size limit, so each one stays as hard to game; there are just four times as many.
#[cfg(not(feature = "short-time"))]
pub const JAR_BURN_INTERVAL: i64 = 900;
#[cfg(feature = "short-time")]
pub const JAR_BURN_INTERVAL: i64 = 30;
#[cfg(not(feature = "short-time"))]
pub const BURN_STUCK_AFTER: i64 = 3 * 86_400;
// Longer than the interval, as in the real build, so burns a normal interval apart use the normal limit.
#[cfg(feature = "short-time")]
pub const BURN_STUCK_AFTER: i64 = 60;

/// Meteora's curve program's signer for moving coins, the same on devnet and mainnet. Settings accounts private to it are
/// the ones a move can use.
pub const METEORA_POOL_AUTHORITY: Pubkey = pubkey!("FhVo3mqL8PW5pH5U2CN4XE33DokiyZnUwuGpH2hmHLuM");
/// Meteora's customizable settings account today, the same address on devnet and mainnet.
pub const METEORA_CUSTOM_SETTINGS: Pubkey = pubkey!("A8gMrEPJkacWkcb3DGwtJwTe16HktSEfvwtuDh2MCtck");

/// A coin may require its address to end in these 4 characters. All zeros means no requirement.
pub const ENDING_LEN: usize = 4;
pub const RESERVED_ENDINGS: [&[u8; ENDING_LEN]; 3] = [b"pump", b"bonk", b"bags"];
pub const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

#[cfg(not(feature = "short-time"))]
pub const DROP_TIMES: [i64; 5] = [86_400, 172_800, 259_200, 604_800, 1_209_600];
#[cfg(feature = "short-time")]
pub const DROP_TIMES: [i64; 5] = [30, 60, 90, 120, 150];
#[cfg(not(feature = "short-time"))]
pub const TREASURY_TIMES: [i64; 4] = [0, 30 * 86_400, 60 * 86_400, 90 * 86_400];
#[cfg(feature = "short-time")]
pub const TREASURY_TIMES: [i64; 4] = [0, 30, 60, 90];
#[cfg(not(feature = "short-time"))]
pub const OPENING_WINDOW: i64 = 1_800;
#[cfg(feature = "short-time")]
pub const OPENING_WINDOW: i64 = 120;
/// How long after the opening window only the lobby's real share can end the 50/50 split. After this, any collection does.
#[cfg(not(feature = "short-time"))]
pub const OPENING_SPLIT_LIMIT: i64 = 3_600;
#[cfg(feature = "short-time")]
pub const OPENING_SPLIT_LIMIT: i64 = 180;

pub const MAX_NAME: usize = 32;
pub const MAX_SYMBOL: usize = 10;
pub const MAX_URI: usize = 200;

pub fn target_for(config: &Pubkey) -> Option<u64> {
    TARGETS.iter().find(|(_, k)| k == config).map(|(t, _)| *t)
}

// Facts the rules rely on, checked when the program compiles, so a wrong edit can't build.
const fn sum(a: &[u64]) -> u64 {
    let (mut i, mut s) = (0, 0);
    while i < a.len() {
        s += a[i];
        i += 1;
    }
    s
}
const fn targets_fill_exactly() -> bool {
    let mut i = 0;
    while i < TARGETS.len() {
        // Buys go in 0.5 SOL steps, so a target must be a multiple of 0.5 SOL to land exactly.
        if TARGETS[i].0 == 0 || TARGETS[i].0 % (SOL / 2) != 0 {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(sum(&DROP_BPS) == BPS, "drops must add up to the whole pot");
const _: () = assert!(sum(&TREASURY_RELEASE_BPS) == BPS, "treasury releases must add up to the whole treasury");
const _: () = assert!(BURN_STUCK_AFTER > BURN_INTERVAL, "the stuck timer must be longer than the burn interval");
const _: () = assert!(BURN_STUCK_AFTER > JAR_BURN_INTERVAL, "the stuck timer must be longer than the jar's burn interval");
#[cfg(not(feature = "short-time"))]
const _: () = assert!(JAR_BURN_INTERVAL == 900 && BURN_INTERVAL == 900, "real builds: the jar and coins burn every 15 minutes");
const _: () = assert!(targets_fill_exactly(), "every target must be a multiple of 0.5 SOL");
#[cfg(not(feature = "short-time"))]
const _: () = assert!(DROP_TIMES[0] == 86_400 && OPENING_WINDOW == 1_800, "a real-time build must use real timers");
