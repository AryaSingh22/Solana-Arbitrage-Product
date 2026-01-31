# Solana Arbitrage Dashboard

A high-performance arbitrage opportunity detection and automated trading system for Solana DEXs, built with Rust + React.

## ✅ Features

### Phase 1: Dashboard
- **Real-time Price Monitoring** - 500ms polling (Raydium, Orca, Jupiter)
- **Arbitrage Detection** - Automatic opportunity identification
- **REST API** - Axum-based endpoints
- **React Dashboard** - Charts, stats, live updates

### Phase 2: Trading Bot
- **Triangular Arbitrage** - Multi-hop path discovery
- **Risk Management** - Circuit breakers, position limits
- **Dry-Run Mode** - Safe testing (default)

## 🚀 Quick Start (Docker)

```bash
# Full stack deployment
docker-compose up --build -d
```

- [Deployment Guide (DEPLOYMENT.md)](DEPLOYMENT.md) - Production setup & Going Live
- [Architecture & Internals (docs/INTERNALS.md)](docs/INTERNALS.md) - Logic behind PathFinder & RiskManager

## 📁 Project Structure

```
solana-arbitrage/
├── crates/             # Rust Backend
│   ├── core/           # Shared library
│   ├── collector/      # Price collector
│   ├── api/            # REST API
│   └── bot/            # Trading bot
├── dashboard/          # React Frontend
├── docker-compose.yml  # Container orchestration
└── DEPLOYMENT.md       # Deployment guide
```

## 🧪 Testing

```bash
cargo test --workspace  # 19 tests
```

## License

MIT
