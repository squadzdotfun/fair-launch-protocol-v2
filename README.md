# Squadz Fair Launch Protocol v2

Fair group launches on [Meteora](https://meteora.ag), on Solana.

A creator opens a lobby for a new coin with a target of 75, 100, 125 or 150 SOL. Members buy into the lobby, all at
the same flat price, from the first buyer to the last. Every buy is a real buy on the coin's Meteora Dynamic Bonding
Curve, made by this program, and a transfer hook keeps the coins locked: nobody outside the lobby can buy before the
coin graduates, so no sniper bots. The last buy completes the curve; the coin then moves to its own Meteora DAMM v2
pool, with locked liquidity, and anyone can trade it.

[Squadz](https://squadz.fun) is the website built on the protocol.

> **Status:** deployed on Solana mainnet (program addresses below). Tested on devnet end to end. **Not independently
> audited. Use at your own risk.**

## Programs

| Program | What it does | Mainnet address |
|---|---|---|
| `squadz-launch` | Lobbies, buys, refunds, launch, HODL drops, project treasury, fees, burns | [`7g2QV5bdgV9LLdhFVJVzPN1bHW8RXk92WgKecQrNAjmw`](https://solscan.io/account/7g2QV5bdgV9LLdhFVJVzPN1bHW8RXk92WgKecQrNAjmw) |
| `squadz-hook` | Transfer hook: a coin only moves while `squadz-launch` runs one of its own instructions | [`J78vo6a5GaE58zxwhy8tw3S8GvMSx5WeuUzBfD4NnN4j`](https://solscan.io/account/J78vo6a5GaE58zxwhy8tw3S8GvMSx5WeuUzBfD4NnN4j) |

## How a launch works

1. **Create.** The creator picks a target and who can join (open, or approval-only). The program creates the coin and
   its Meteora curve at once. The coin has no mint or freeze power, and its details can't be changed.
2. **Buy.** Members buy 0.5, 1, 1.5 or 2 SOL, and can top up to 2 SOL in total per wallet. Each buy needs a join pass
   signed by the lobby's pass key. Everyone gets the same amount of coins per SOL.
3. **Refund.** Any time before graduation, a member can hand their coins back and get what they paid, minus
   Meteora's curve fee both ways and 0.01 SOL. A lobby with no buy for 30 days times out; refunds stay open.
4. **Launch.** The last buy completes the curve, Meteora removes the hook, and the coin moves to its pool. The
   address stays the same.
5. **After launch.** Five HODL drops over two weeks for members who keep at least 75% of their launch coins; an
   optional project treasury released over 90 days; trading fees split between the creator (or a buy-and-burn of the
   coin) and buying and burning $SQUADZ.

## Build

Requires the Solana CLI 2.3, Anchor 0.31.1 and Docker (for the verifiable build).

```bash
# the exact program deployed on mainnet (same bytes every time)
solana-verify build --library-name squadz_launch -- --features mainnet
solana-verify build --library-name squadz_hook -- --features mainnet
```

Every build must pick `--features devnet` or `--features mainnet`; the mainnet build refuses to compile unless its
addresses are set (`programs/squadz-launch/src/constants.rs`, `programs/squadz-hook/src/lib.rs`).

## Security

See [SECURITY.md](SECURITY.md). Report issues to **security@squadz.fun**.

## License

MIT, see [LICENSE](LICENSE).
