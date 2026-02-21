# Solana Arbitrage Engine v1.0 (Production)

A high-performance, modular arbitrage trading system for Solana, built with Rust. This engine is designed for low-latency execution, capital efficiency via flash loans, and safety through multi-tier risk management.

## ✅ Key Features

### ⚡ Core Performance
- **Async Architecture**: Built on Tokio for non-blocking I/O.
- **WebSocket Streaming**: Real-time price updates (500ms latency).
- **Zero-Copy Parsing**: `simd-json` integration for ultra-fast data handling.
- **Memory-Mapped Cache**: Shared price state across threads.

### 💸 Execution & Liquidity
- **Flash Loans**: Integrated Solend & Marginfi for leverage without collateral.
- **Address Lookup Tables (ALTs)**: V0 Transaction support for complex, multi-hop bundles.
- **Jito MEV Integration**: Bundle submission to bypass public mempool and prevent sandwich attacks.
- **Multi-DEX Support**: Raydium, Orca, Jupiter, Lifinity, Meteora, Phoenix.

### 🧠 Strategy Engine
- **Arbitrage**: Triangular and cyclic path discovery.
- **Statistical**: Mean reversion and volatility-based triggers.
- **Latency**: High-frequency opportunities across DEXs.

### 🛡️ Risk Management
- **Circuit Breakers**: Auto-pause on significant daily losses or consecutive failures.
- **Volatility Sizing**: Dynamic position sizing based on market conditions.
- **VaR Calculator**: Real-time Value-at-Risk monitoring.

## 🔄 System Workflow

1.  **Scan**: `PriceFetcher` streams quotes from RPC/WebSocket.
2.  **Detect**: `PathFinder` identifies profitable cycles (e.g., SOL -> USDC -> BONK -> SOL).
3.  **Evaluate**: `StrategyEngine` filters opportunities based on profit thresholds and risk checks.
4.  **Optimise**: `FlashLoanBuilder` wraps the trade with a flash loan and optimizes compute units.
5.  **Execute**: `JitoClient` sends the bundle directly to validators.
6.  **Log**: `MetricsCollector` records performance to Prometheus/TimescaleDB.

## 🚀 Quick Start

### Prerequisites
- Rust (1.75+)
- Solana CLI
- Paid RPC Provider (Helius, QuickNode, Triton) recommended for mainnet.

### Installation
1.  **Clone the repository**:
    ```bash
    git clone https://github.com/AryaSingh22/Solana-Arbitrage-Product.git
    cd Solana-Arbitrage-Product
    ```

2.  **Configure Environment**:
    ```bash
    cp .env.example .env
    # Edit .env:
    # SOLANA_RPC_URL=https://...
    # PRIVATE_KEY=...
    # ENABLE_FLASH_LOANS=true
    ```

3.  **Build (Release)**:
    ```bash
    cargo build -p solana-arb-bot --release
    ```

### Usage

**Dry Run (Simulation)**:
```bash
# Windows
$env:DRY_RUN="true"; ./target/release/bot.exe

# Linux/Mac
DRY_RUN=true ./target/release/bot
```

**Live Trading**:
```bash
# Ensure DRY_RUN=false in .env
./target/release/bot.exe
```

## 🧪 Test Validation

The system has passed a comprehensive test suite covering all critical modules:

| Module | Tests Passed | Description |
| :--- | :---: | :--- |
| **Arbitrage** | 5/5 | Profit calculation, path detection, triangular cycles. |
| **Risk** | 5/5 | Circuit breakers, position limits, trade approval logic. |
| **Execution** | 4/4 | Flash loan builder, Devnet integration, V0 transactions. |
| **Types/Math** | 6/6 | Price data scaling, spread calculations, token parsing. |
| **Config** | 5/5 | Environment loading, default values. |

**Total Tests Passed: 25** ✅

## 📚 Documentation
- **[Deployment Guide](docs/DEPLOYMENT.md)**: Detailed VPS and Docker setup instructions.
- **[Internal Architecture](docs/INTERNALS.md)**: Deep dive into the pathfinding and risk engine.
- **[Easy Tutorial](docs/EASY_TUTORIAL.md)**: Beginner-friendly guide for running simulated trades.

## 📁 Project Structure

```
crates/
├── bot/            # Main entry point & execution loop
├── core/           # Shared logic, pricing, risk, pathfinding
├── api/            # (Optional) WebSocket API for frontend
├── flash-loans/    # Integration with lending protocols
├── dex-plugins/    # Connectors for specific DEXs
└── strategies/     # Alpha logic implementation
```

## ⚠️ Disclaimer

This software is for educational purposes. Cryptocurrency trading involves high risk. The authors are not responsible for any financial losses incurred while using this bot. Use at your own risk.

## License

MIT
