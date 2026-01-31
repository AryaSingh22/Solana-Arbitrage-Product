import { OpportunitiesTable } from './components/OpportunitiesTable';
import { PriceComparison } from './components/PriceComparison';
import { StatusBar } from './components/StatusBar';
import { ProfitChart } from './components/ProfitChart';
import { StatsCards } from './components/StatsCards';
import { SimulationBanner } from './components/SimulationBanner';
import './App.css';

function App() {
  return (
    <div className="app">
      <SimulationBanner />
      <header className="app-header">
        <div className="logo">
          <span className="logo-icon">⚡</span>
          <h1>Solana Arbitrage Dashboard</h1>
        </div>
        <p className="subtitle">Real-time arbitrage opportunity detection across Solana DEXs</p>
      </header>

      <main className="app-main">
        {/* Stats Overview */}
        <section className="section">
          <StatsCards />
        </section>

        {/* Charts Row */}
        <section className="section">
          <ProfitChart />
        </section>

        {/* Live Data */}
        <section className="section">
          <OpportunitiesTable />
        </section>

        <section className="section">
          <PriceComparison />
        </section>
      </main>

      <footer className="app-footer">
        <StatusBar />
      </footer>
    </div>
  );
}

export default App;
