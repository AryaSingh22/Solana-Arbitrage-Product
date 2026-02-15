# ArbEngine-Pro Troubleshooting Guide

## Bot Not Finding Opportunities

**Symptoms**: Bot runs but logs show 0 opportunities

**Causes**:
1. RPC provider too slow (>500ms latency)
2. MIN_PROFIT_BPS too high
3. Not enough DEXs configured
4. Slippage tolerance too low

**Solutions**:
```bash
# Test RPC latency
curl -X POST $SOLANA_RPC_URL -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getSlot"}' --write-out "\nTime: %{time_total}s\n"
# Should be <0.5s

# Temporarily lower MIN_PROFIT_BPS
MIN_PROFIT_BPS=20 cargo run

# Check which DEXs are active
grep "Registered DEX" logs/arbengine.log
```

## Trades Failing

**Symptoms**: Opportunities detected but trades fail

**Common Errors & Fixes**:

### "Insufficient funds"
- Check balance: `solana balance`
- Need at least 0.1 SOL for fees
- If using flash loans: Disable until implemented

### "Transaction timeout"
- RPC provider overloaded
- Increase timeout: `TRANSACTION_TIMEOUT_SECONDS=60`
- Or switch to premium RPC

### "Slippage exceeded"
- Market moved too fast
- Increase: `SLIPPAGE_BPS=150` (1.5%)
- Or reduce position size

### "Circuit breaker open"
- Too many consecutive failures
- Wait for timeout period (5 minutes default)
- Or adjust: `CIRCUIT_BREAKER_TIMEOUT_SECONDS=300`

## High Gas Costs

**Symptoms**: Trades work but barely profitable due to fees

**Solutions**:
- Implement ALT (Phase 11, Task 2)
- Optimize compute units
- Increase MIN_PROFIT_BPS to account for fees
- Use Jito for better inclusion

## Memory Leaks

**Symptoms**: Bot slows down over time, memory usage grows

**Causes**:
- Not clearing old price data
- Keeping too many historical trades in memory

**Solutions**:
```rust
// Add periodic cleanup in main loop
if last_cleanup.elapsed() > Duration::from_hours(1) {
    price_cache.clear_old_entries();
    trade_history.truncate(1000); // Keep last 1000 only
    last_cleanup = Instant::now();
}
```

## Circuit Breaker Triggering Too Often

**Symptoms**: Trading stops frequently

**Solutions**:
- Review failure reasons in logs
- Increase threshold: `MAX_CONSECUTIVE_LOSSES=10`
- Fix underlying issues (RPC, slippage, etc.)
- Consider disabling temporarily (not recommended)
