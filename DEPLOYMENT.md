# Production Deployment Guide

This guide explains how to deploy the Solana Arbitrage system using Docker and how to handle the key signing limitations on Windows.

## 🐳 Docker Deployment (Recommended)

The easiest way to run the entire stack (Database, Cache, API, Collector, Bot) is via Docker Compose.

### Prerequisites
- Docker Engine & Docker Compose installed
- Solana RPC URL (optional, defaults to public mainnet)
- Private Key (for bot execution)

### 1. Configuration
Create a `.env` file from the example:
```bash
cp .env.example .env
```
Edit `.env` to set your values:
```bash
PRIVATE_KEY="[YOUR_BASE58_PRIVATE_KEY]"  # Required for Bot
SOLANA_RPC_URL="https://api.mainnet-beta.solana.com"
```

### 2. Build & Run
```bash
docker-compose up --build -d
```
This command starts:
- **PostgreSQL + TimescaleDB** (Port 5432)
- **Redis** (Port 6379)
- **API Server** (Port 8080)
- **Collector Service** (Background)
- **Trading Bot** (Background)

### 3. Verify Services
Check logs:
```bash
docker-compose logs -f
```
Access API:
```bash
curl http://localhost:8080/health
```

---

## ⚠️ Windows vs. Linux/WSL

### The Issue
The `solana-sdk` Rust crate has known file locking availability issues when compiling on native Windows. This prevents the bot from building with real transaction signing capabilities on Windows.

### The Solution: Simulation Mode
On Windows, the bot defaults to **Simulation Mode**:
- ✅ Fetches live quotes from Jupiter
- ✅ Simulates transaction building
- ❌ Does NOT sign or send real transactions

### How to Enable Real Trading
To run the bot with real trading capabilities, you must deploy on a **Linux** environment (e.g., Ubuntu, Debian, or **WSL 2** on Windows).

1. **Install WSL 2** (if on Windows)
   ```powershell
   wsl --install
   ```
2. **Clone repo into WSL**
3. **Uncomment Solana SDK dependencies**
   
   First in root `Cargo.toml`:
   ```toml
   # Cargo.toml
   solana-sdk = "1.18"
   solana-client = "1.18"
   ```

   Then in `crates/bot/Cargo.toml`:
   ```toml
   # crates/bot/Cargo.toml
   solana-sdk = { workspace = true }
   solana-client = { workspace = true }
   ```
4. **Build & Run**
   ```bash
   cargo run --bin bot
   ```

## 🚀 Going Live: Production Best Practices

To run this bot profitably on mainnet, consider the following:

### 1. RPC Provider
Do **NOT** use the public `api.mainnet-beta.solana.com` for trading. It is rate-limited and slow.
- **Recommended**: [Helius](https://helius.xyz), [Triton](https://triton.one), or [QuickNode](https://quicknode.com).
- **Configuration**: Set `SOLANA_RPC_URL` in your `.env`.

### 2. Transaction Landing (Priority Fees)
Solana network congestion requires **Compute Unit (CU) optimalization** and **Priority Fees**.
- The current simulation uses a static configuration.
- **Upgrade**: Implement dynamic fee estimation (e.g., fetch "high" priority fee tier from RPC) to ensure your arbitrage transactions land in the next block.

### 3. Latency
- Run your bot node as close to the validator leaders as possible (e.g., AWS us-east-1, Tokyo, or Amsterdam depending on leader schedule).
- Use **Geyser Plugins** (gRPC) for faster account updates instead of polling HTTP.

### 4. Security
- Create a dedicated "Trade Wallet" with limited funds.
- Never store large amounts of SOL in the bot's hot wallet.
- Rotate private keys periodically.

## 🛠 Manual Deployment (Linux)

If avoiding Docker, run services individually:

1. **Infrastructure**: `docker-compose up -d postgres redis`
2. **Migrations**: `sqlx migrate run`
3. **Services**:
   ```bash
   # Terminal 1
   cargo run --release --bin collector
   
   # Terminal 2
   cargo run --release --bin api
   
   # Terminal 3
   cargo run --release --bin bot
   ```
