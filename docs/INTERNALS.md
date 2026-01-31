# System Architecture & Internals

This document details the internal logic of the Solana Arbitrage Bot, specifically the arbitrage detection engine and risk management system.

## 1. Triangular Arbitrage Engine (`pathfinder.rs`)

The bot isn't limited to simple A->B->A swaps. It uses a **Graph-Based Depth-First Search (DFS)** to find profitable multi-hop paths.

### How it works:
1.  **Graph Construction**: 
    - Vertices = Tokens (SOL, USDC, RAY, etc.)
    - Edges = Active DEX Pools (Raydium, Orca, Jupiter)
    - Weights = Price / Exchange Rate

2.  **Path Discovery**:
    - The `PathFinder` explores paths starting from a *quote token* (e.g., USDC).
    - It recurses up to `MAX_HOPS` (default: 3) to find a cycle back to the start token.
    - **Example Path**: `USDC` -> `SOL` (Raydium) -> `RAY` (Orca) -> `USDC` (Jupiter).

3.  **Profit Calculation**:
    - Calculating the cumulative product of exchange rates along the path.
    - Deducting estimated fees (network fee + DEX trading fees).
    - `Net Profit % = (Final Amount / Initial Amount) - 1`

## 2. Risk Management (`risk.rs`)

To prevent catastrophic losses, the bot implements a robust `RiskManager` that acts as a middleware before any trade execution.

### Components:

#### A. Position Sizing (Kelly Criterion - Simplified)
Instead of betting the entire wallet, the bot calculates an optimal size based on:
- **Confidence**: `Profit %` (Higher profit = larger size)
- **Limits**: Never exceeds `max_position_size` config.

#### B. Circuit Breakers
- **Daily Loss Limit**: If `daily_pnl` drops below `-$100` (configurable), the bot enters a **PAUSED** state.
- **Cooldowns**: After a failed trade, specific pairs are blacklisted for a short duration to avoid repeating mistakes against toxic flow.

#### C. Exposure Limits
- **Max Total Exposure**: Caps the total USD value of all open trades (relevant for future async execution).

## 3. Bot Lifecycle (`bot/src/main.rs`)

The `run_trading_loop` functions as the heartbeat:
1.  **Poll (500ms)**: Fetch latest prices from all DEXs.
2.  **Detect**: Run `ArbitrageDetector` (Simple) and `PathFinder` (Triangular).
3.  **Evaluate**: Pass best opportunity to `RiskManager`.
4.  **Execute**:
    - **Dry Run**: Log outcome, simulate profit/loss.
    - **Live**: Request quote & swap instructions from Jupiter API (HTTP), sign and send.
