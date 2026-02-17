# Monitoring & Alerting Guide

## Overview
ArbEngine-Pro uses a Prometheus + Grafana stack for real-time monitoring and alerting.

## Metrics
The bot exposes metrics at `http://localhost:9090/metrics`.

### Key Metrics
| Metric Name | Type | Description |
|---|---|---|
| `arb_trades_total` | Counter | Total number of executed trades |
| `arb_profit_usd_total` | Counter | Total profit in USD |
| `arb_latency_seconds` | Histogram | End-to-end latency distribution |
| `arb_price_updates_total` | Counter | Number of price updates processed |
| `arb_circuit_breaker_status` | Gauge | 0=Closed (Normal), 1=Open (Halted) |

## Grafana Dashboard
A standard dashboard is provided in `grafana/dashboard.json`.

### Panels
- **Profit/Loss (P&L)**: Real-time P&L chart.
- **Trade Volume**: Trades per minute.
- **Latency Heatmap**: Execution speed visualization.
- **DEX Success Rate**: Success rate per DEX (Orca, Raydium, etc.).
- **System Health**: CPU/Memory usage and uptime.

## Alert Rules
Configured in `prometheus/alerts.yml`.

### Critical Alerts (PagerDuty/Discord)
- **Circuit Breaker Open**: Bot has halted due to consecutive losses.
- **Low Balance**: SOL balance < 0.1 SOL.
- **High Latency**: 99th percentile latency > 5s.
- **Price Staleness**: No price updates for > 30s.

### Warning Alerts (Discord/Slack)
- **Missed Opportunities**: Opportunities detected but not executed (e.g. low profit).
- **API Rate Limits**: Rate limit errors detected.

## SLA targets
- **Uptime**: 99.9%
- **Execution Latency**: < 2s (99th percentile)
- **Profitability**: > 0.5% net profit per trade
