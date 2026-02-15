# ArbEngine-Pro Deployment Checklist

## Pre-Deployment (1 Day Before)

- [ ] Backup current configuration
- [ ] Test on devnet for 24 hours
- [ ] Review logs for errors
- [ ] Check balance (min 1 SOL)
- [ ] Verify RPC provider is premium (not public)
- [ ] Test emergency stop mechanism
- [ ] Set up monitoring alerts (Telegram/Discord)
- [ ] Verify circuit breaker settings

## Configuration Review

- [ ] `DRY_RUN=false` (for live trading)
- [ ] `ENABLE_FLASH_LOANS=false` (until implemented)
- [ ] `MAX_POSITION_SIZE` set appropriately
- [ ] `MIN_PROFIT_BPS` ≥ 50 (0.5% minimum)
- [ ] `CIRCUIT_BREAKER_ENABLED=true`
- [ ] `MAX_DAILY_LOSS` set (recommend $500 initially)
- [ ] Monitoring ports open (8080, 9090)

## Deployment Day

- [ ] Run pre-flight checks
- [ ] Start with small capital ($100-500)
- [ ] Monitor for first hour continuously
- [ ] Check balance every 30 minutes
- [ ] Review logs every hour
- [ ] Test emergency stop

## Post-Deployment (First Week)

- [ ] Monitor daily P&L
- [ ] Check success rate (target >60%)
- [ ] Review failed trades
- [ ] Adjust parameters if needed
- [ ] Gradually increase capital
- [ ] Document any issues

## Emergency Procedures

If things go wrong:
1. Create `.kill` file (immediate stop)
2. OR: `curl -X POST http://localhost:8080/emergency/stop`
3. Check logs: `tail -100 logs/errors.log`
4. Review recent trades in dashboard
5. Don't panic - circuit breaker prevents major losses
