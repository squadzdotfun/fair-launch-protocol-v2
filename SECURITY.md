# Security

**Not audited by an independent firm.** This is the team's own review.

## Reporting

Email **security@squadz.fun**. Please give us a reasonable time to fix an issue before you publish it, and don't
use it against real users or their funds.

## Trust model

- **No admin switches.** The program has no pause, no fee switch and no owner key. Every rule is fixed in the code.
- **Upgrades.** The upgrade authority of both programs is held by the team's 2-of-4 Squads multisig. Until it is
  removed, the multisig could change the programs, including how funds held by them are handled.
- **Funds** sit in program addresses with no private key: each lobby's vault, its member records, the coin reserve,
  the burn jar and the operations account. Anything anyone can call can only move funds to addresses fixed in state.
- **Join passes.** Each lobby names its pass key at creation; whoever holds that key decides who may buy into that
  lobby (and nothing else). A stolen pass key lets wallets into those lobbies but can't move any funds.
- **Meteora.** Coins use Meteora's Dynamic Bonding Curve (transfer-hook pools) and DAMM v2. The program checks every
  Meteora account it uses against fixed program addresses and the lobby's own settings.

## Known risks and limitations

1. **Upgrade authority** (above).
2. **Meteora changes.** If Meteora adds new customizable pool settings, someone could move a launched coin to a pool
   the program doesn't recognise; that coin's fees then can't be collected and its burns stop. Fixing it needs an
   upgrade.
3. **Join passes are reusable until they expire.** A pass covers the wallet and lobby, not one buy; keep pass
   lifetimes short.
4. **Lobbies made outside Squadz.** Anyone can create a lobby on the program with their own pass key and settings.
   Websites should only list lobbies whose pass key is their own.
5. **Holdings are sampled.** The 75% rule for HODL drops is checked by spot checks; a sale and buy-back between two
   checks may go unseen. Websites should check on every member sale they detect.
6. **Sybils.** One person can buy with several wallets; pass keys decide who gets passes.
7. **Dust.** Rounding dust stays with the lobby's accounts.
8. **Setup.** Fee collection needs the burn jar's wrapped SOL account to exist; anyone can create it.

All arithmetic is checked (release builds have overflow checks on) and divisions round down, so payouts never exceed
what the program holds.
