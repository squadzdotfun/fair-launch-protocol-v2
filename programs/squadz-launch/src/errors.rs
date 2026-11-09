use anchor_lang::prelude::*;

#[error_code]
pub enum LaunchError {
    #[msg("Account doesn't match")]
    AccountMismatch,
    #[msg("Target isn't one of the allowed targets")]
    BadTarget,
    #[msg("Name, ticker or link is too long")]
    TooLong,
    #[msg("Treasury can be at most 20%")]
    BadTreasury,
    #[msg("A join pass key is required")]
    PassKeyRequired,
    #[msg("This lobby isn't open")]
    NotOpen,
    #[msg("This lobby has had no buys for 30 days")]
    TimedOut,
    #[msg("Buys must be 0.5, 1, 1.5 or 2 SOL")]
    BadAmount,
    #[msg("Donation must be 0 or at least 0.001 SOL")]
    BadDonation,
    #[msg("Join pass missing")]
    PassMissing,
    #[msg("Join pass is wrong")]
    BadPass,
    #[msg("Join pass has expired")]
    PassExpired,
    #[msg("Refunds stop when the curve completes")]
    AlreadyLaunched,
    #[msg("Only the creator can do this")]
    NotCreator,
    #[msg("The lobby hasn't launched yet")]
    NotLaunched,
    #[msg("Already claimed")]
    AlreadyClaimed,
    #[msg("Math overflow")]
    Overflow,
    #[msg("The coin hasn't moved to its pool yet")]
    NotMoved,
    #[msg("Already settled")]
    AlreadySettled,
    #[msg("Leftover coins haven't reached the reserve yet")]
    NotFinalized,
    #[msg("Not due yet")]
    TooEarly,
    #[msg("Nothing to do")]
    NothingToDo,
    #[msg("This coin's fees don't go to its own burn")]
    WrongFeeChoice,
    #[msg("Lobby isn't closed")]
    NotClosed,
    #[msg("This member can't close yet")]
    RecordInUse,
    #[msg("Meteora settings aren't the agreed ones")]
    WrongMeteoraSettings,
    #[msg("The coin has a mint or freeze power, or the wrong supply")]
    CoinNotFixed,
    #[msg("Fee choice must be 0 or 1")]
    BadFeeChoice,
    #[msg("This wallet can't be used")]
    BadWallet,
    #[msg("Name and ticker can't be empty")]
    EmptyName,
    #[msg("Treasury must be in whole percents")]
    TreasuryNotWhole,
    #[msg("Address ending must be 4 base58 characters and not a reserved one")]
    BadEnding,
    #[msg("The coin's address doesn't end in the chosen characters")]
    WrongEnding,
    #[msg("Someone is still eligible for drops")]
    StillEligible,
    #[msg("Burn got fewer coins than its limit")]
    TooFewCoins,
    #[msg("That buy is bigger than what's left of the target")]
    BuyTooBig,
    #[msg("The curve doesn't match the lobby's total")]
    CurveMismatch,
    #[msg("A wallet can put in 2 SOL at most")]
    WalletLimit,
    #[msg("The creator's own seat is only taken in the transaction that creates the lobby")]
    SeatTooLate,
}
