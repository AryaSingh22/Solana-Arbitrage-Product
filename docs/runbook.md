# ArbEngine-Pro Operations Runbook

## Quick Reference

| Action | Command |
|--------|---------|
| Build | `cargo build --release` |
| Test | `cargo test --workspace` |
| Dry run | `DRY_RUN=true cargo run --bin bot` |
| Live trading | `DRY_RUN=false cargo run --bin bot --release` |
| Kill switch | `touch .kill` (create file in project root) |
| Health check | `curl http://localhost:8080/health` |
| Status | `curl http://localhost:8080/status` |
| Metrics | `curl http://localhost:9090/metrics` |

## Startup

### 1. Environment Setup

Copy `.env.example` to `.env` and configure:

```bash
# Required
SOLANA_RPC_URL=https://api.mainnet-beta.solana.com
WALLET_PRIVATE_KEY=<base58-encoded>

# Optional
DRY_RUN=true
MIN_PROFIT_THRESHOLD=0.5
PRIORITY_FEE_MICRO_LAMPORTS=100000
SLIPPAGE_BPS=50
USE_JITO=false
JITO_BLOCK_ENGINE_URL=https://mainnet.block-engine.jito.wtf
JITO_TIP_LAMPORTS=100000
```

### 2. Pre-flight Checks

The bot automatically runs pre-flight safety checks on startup:
- RPC connectivity and latency
- Wallet balance verification
- Configuration validation

### 3. Start the Bot

```bash
# Development / dry run
cargo run --bin bot

# Production
cargo run --bin bot --release
```

## Monitoring

### Health Endpoints

| Endpoint | Port | Description |
|----------|------|-------------|
| `/health` | 8080 | Simple liveness check |
| `/status` | 8080 | Detailed status (trades, circuit breaker, balance) |
| `/metrics` | 9090 | Prometheus-format metrics |

### Key Metrics

- `trades_attempted_total` — Total trade attempts
- `trades_successful_total` — Successful trades
- `trades_failed_total` — Failed trades
- `opportunity_profit` — Profit distribution
- `trade_execution_time_seconds` — Execution latency
- `price_fetch_latency_seconds` — Price collection latency

### Audit Logs

Trade audit logs are written to `data/audit.jsonl` in JSONL format. Each line is a JSON object with:
- `timestamp`, `category` (TRADE/RISK/SYSTEM), `action`, `resource`, `result`, `details`

## Emergency Procedures

### Graceful Shutdown

1. Create a `.kill` file in the project root: `touch .kill`
2. The bot will detect it within 1 tick (500ms) and shut down gracefully
3. Remove the file after shutdown: `rm .kill`

### Circuit Breaker

The circuit breaker automatically pauses trading when:
- Consecutive losses exceed `max_consecutive_losses` (default: 5)
- Daily P&L exceeds `max_daily_loss` (default: $500)
- VaR exceeds `var_limit_percent` (default: 2%)

To manually reset, restart the bot.

### Critical Alert Response

1. **Low balance alert**: Check wallet balance, add funds if needed
2. **Circuit breaker open**: Review recent trades in audit log
3. **Flash loan failure**: Check Solend reserve liquidity
4. **RPC timeout**: Check RPC provider status, consider switching providers

## Configuration Hot-Reload

Edit `config/trading_config.json` with new parameters. The `ConfigManager` supports hot-reload — changes take effect without restart.

**Always validate** config changes. Invalid configs are rejected with an error, keeping the previous valid config active.

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| No opportunities found | Threshold too high | Lower `MIN_PROFIT_THRESHOLD` |
| All trades rejected | Risk limits too tight | Adjust `config/trading_config.json` risk section |
| RPC timeouts | Provider overloaded | Switch to paid RPC (Helius, QuickNode) |
| Port already in use | Another instance running | Kill previous process or change port |
| High latency | No parallel fetching | Enable `enable_parallel_fetching` in config |

## Backup & Recovery

- **Trade history**: `data/history-live.jsonl` / `data/history-sim.jsonl`
- **Audit log**: `data/audit.jsonl`
- **Configuration**: `config/trading_config.json`, `config/solend_reserves.json`

Back up these files regularly. All are append-only JSONL and can be safely copied while the bot is running.
