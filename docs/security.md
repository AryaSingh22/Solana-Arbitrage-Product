# Security Protocols

## Threat Model
The system operates in a high-risk financial environment. Primary threats:
1. **Key Compromise**: Unauthorized access to the trading wallet.
2. **API Key Leakage**: Exposure of RPC or compiled secrets.
3. **Malicious Code Injection**: Supply chain attacks.
4. **Flash Loan Exploits**: Smart contract vulnerabilities.

## Key Management
- **Private Keys**: NEVER stored in code. Loaded from `PRIVATE_KEY` env var at runtime.
- **Zeroization**: Sensitive memory is cleared when possible (Note: mitigated by redaction in logs if zeroization unavailable).
- **Environment Isolation**: Production keys available only in secure CI/CD environments.

## Incident Response Plan
1. **Detection**: Alert triggers (e.g. "Circuit Breaker Open" or "Unauthorized Transfer").
2. **Containment**: 
   - Kill switch: Create `.kill` file in root or trigger via API.
   - Revoke keys: Transfer remaining funds to cold wallet immediately.
3. **Eradication**: Identify vulnerability (code bug, leaked key).
4. **Recovery**: Patch system, rotate keys, restart services.
5. **Post-Mortem**: Document root cause and preventative measures.

## Audit Procedures
- **Pre-Flight**: Run `cargo audit` to check dependencies.
- **Code Review**: All changes to `execution.rs` and `wallet.rs` require 2-person review.
- **Logs**: Review `audit.log` daily for anomalous activity.

## Access Control
- **RPC Access**: Whitelist IP addresses for RPC endpoints.
- **Server Access**: SSH via key-only, disable root login.
