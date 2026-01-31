import { useState, useEffect } from 'react';
import './SimulationBanner.css';

interface StatusData {
    dry_run: boolean;
    bot_running: boolean;
    simulated_pnl: number;
    simulated_trades: number;
}

export function SimulationBanner() {
    const [status, setStatus] = useState<StatusData | null>(null);

    useEffect(() => {
        const fetchStatus = async () => {
            try {
                const res = await fetch('/api/status');
                const data = await res.json();
                if (data.success) {
                    setStatus(data.data);
                }
            } catch (err) {
                console.error('Failed to fetch status:', err);
            }
        };

        fetchStatus();
        const interval = setInterval(fetchStatus, 5000);
        return () => clearInterval(interval);
    }, []);

    if (!status?.dry_run) {
        return null; // Don't show banner in LIVE mode
    }

    return (
        <div className="simulation-banner">
            <div className="simulation-banner-content">
                <span className="simulation-icon">⚠️</span>
                <span className="simulation-text">
                    <strong>SIMULATION MODE</strong> — No real trades are being executed
                </span>
                <div className="simulation-stats">
                    <span className="stat">
                        Simulated P&L: <strong>${status.simulated_pnl.toFixed(2)}</strong>
                    </span>
                    <span className="stat">
                        Trades: <strong>{status.simulated_trades}</strong>
                    </span>
                </div>
            </div>
        </div>
    );
}
