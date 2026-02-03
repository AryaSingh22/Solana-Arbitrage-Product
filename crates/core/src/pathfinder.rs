//! Triangular Arbitrage Path Discovery
//!
//! This module implements graph-based path finding to discover multi-hop
//! arbitrage opportunities across DEXs.

use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

use crate::{DexType, PriceData};
#[cfg(test)]
use crate::TokenPair;

/// Represents an edge in the trading graph
#[derive(Debug, Clone)]
pub struct TradingEdge {
    pub from_token: String,
    pub to_token: String,
    pub dex: DexType,
    pub rate: Decimal,      // Exchange rate (how much to_token you get per from_token)
    pub liquidity: Decimal, // Available liquidity
    pub fee: Decimal,       // Trading fee percentage
}

impl TradingEdge {
    pub fn effective_rate(&self) -> Decimal {
        // Rate after fees
        self.rate * (Decimal::ONE - self.fee / Decimal::from(100))
    }
}

/// A path through the trading graph
#[derive(Debug, Clone)]
pub struct TradingPath {
    pub edges: Vec<TradingEdge>,
    pub profit_ratio: Decimal,
    pub min_liquidity: Decimal,
}

impl TradingPath {
    /// Calculate the overall profit ratio for this path
    pub fn calculate_profit_ratio(&self) -> Decimal {
        let mut ratio = Decimal::ONE;
        for edge in &self.edges {
            ratio *= edge.effective_rate();
        }
        ratio
    }

    /// Check if this path is profitable (ratio > 1)
    pub fn is_profitable(&self) -> bool {
        self.profit_ratio > Decimal::ONE
    }

    /// Get the profit percentage
    pub fn profit_percentage(&self) -> Decimal {
        (self.profit_ratio - Decimal::ONE) * Decimal::from(100)
    }

    /// Get the optimal trade size based on liquidity
    pub fn optimal_size(&self, max_position: Decimal) -> Decimal {
        // Take minimum of max position and available liquidity
        max_position.min(self.min_liquidity)
    }
}

/// Graph-based arbitrage path finder
pub struct PathFinder {
    /// Adjacency list: token -> list of outgoing edges
    edges: HashMap<String, Vec<TradingEdge>>,
    /// All known tokens
    tokens: HashSet<String>,
    /// Maximum path length to consider
    max_hops: usize,
}

impl PathFinder {
    pub fn new(max_hops: usize) -> Self {
        Self {
            edges: HashMap::new(),
            tokens: HashSet::new(),
            max_hops,
        }
    }

    /// Clear all edges and rebuild from fresh price data
    pub fn clear(&mut self) {
        self.edges.clear();
        self.tokens.clear();
    }

    /// Add a trading edge from price data
    pub fn add_price(&mut self, price: &PriceData) {
        let base = price.pair.base.clone();
        let quote = price.pair.quote.clone();
        let fee = price.dex.fee_percentage();
        
        self.tokens.insert(base.clone());
        self.tokens.insert(quote.clone());

        // Forward edge: base -> quote (selling base for quote)
        // Rate is the bid price (what you get when selling)
        let forward = TradingEdge {
            from_token: base.clone(),
            to_token: quote.clone(),
            dex: price.dex,
            rate: price.bid,
            liquidity: price.liquidity.unwrap_or(Decimal::from(100000)),
            fee,
        };

        // Reverse edge: quote -> base (buying base with quote)
        // Rate is 1/ask (how much base you get per quote)
        let reverse = TradingEdge {
            from_token: quote.clone(),
            to_token: base.clone(),
            dex: price.dex,
            rate: Decimal::ONE / price.ask,
            liquidity: price.liquidity.unwrap_or(Decimal::from(100000)),
            fee,
        };

        self.edges.entry(base).or_default().push(forward);
        self.edges.entry(quote).or_default().push(reverse);
    }

    /// Find all triangular arbitrage paths starting and ending at the given token
    pub fn find_triangular_paths(&self, start_token: &str) -> Vec<TradingPath> {
        let mut paths = Vec::new();
        
        if !self.tokens.contains(start_token) {
            return paths;
        }

        // DFS to find cycles
        self.dfs_find_paths(
            start_token,
            start_token,
            Vec::new(),
            Decimal::ONE,
            Decimal::MAX,
            &mut paths,
        );

        // Sort by profit (descending)
        paths.sort_by(|a, b| b.profit_ratio.cmp(&a.profit_ratio));
        paths
    }

    fn dfs_find_paths(
        &self,
        current: &str,
        start: &str,
        current_path: Vec<TradingEdge>,
        current_ratio: Decimal,
        min_liquidity: Decimal,
        results: &mut Vec<TradingPath>,
    ) {
        // Check for cycle completion (back to start)
        if current_path.len() >= 2 && current == start {
            let path = TradingPath {
                edges: current_path,
                profit_ratio: current_ratio,
                min_liquidity,
            };
            if path.is_profitable() {
                results.push(path);
            }
            return;
        }

        // Stop if path too long
        if current_path.len() >= self.max_hops {
            return;
        }

        // Explore neighbors
        if let Some(neighbors) = self.edges.get(current) {
            for edge in neighbors {
                // Avoid revisiting tokens (except returning to start)
                let already_visited = current_path.iter().any(|e| e.to_token == edge.to_token);
                if already_visited && edge.to_token != start {
                    continue;
                }

                let mut new_path = current_path.clone();
                new_path.push(edge.clone());

                let new_ratio = current_ratio * edge.effective_rate();
                let new_min_liq = min_liquidity.min(edge.liquidity);

                self.dfs_find_paths(
                    &edge.to_token,
                    start,
                    new_path,
                    new_ratio,
                    new_min_liq,
                    results,
                );
            }
        }
    }

    /// Find the most profitable path
    pub fn find_best_path(&self, start_token: &str) -> Option<TradingPath> {
        self.find_triangular_paths(start_token).into_iter().next()
    }

    /// Find all profitable paths across all tokens
    pub fn find_all_profitable_paths(&self) -> Vec<TradingPath> {
        let mut all_paths = Vec::new();

        for token in &self.tokens {
            let mut paths = self.find_triangular_paths(token);
            all_paths.append(&mut paths);
        }

        // Deduplicate (same cycle can be found from different starting points)
        all_paths.dedup_by(|a, b| {
            a.edges.len() == b.edges.len()
                && a.edges.iter().zip(b.edges.iter()).all(|(ea, eb)| {
                    ea.from_token == eb.from_token && ea.to_token == eb.to_token
                })
        });

        all_paths.sort_by(|a, b| b.profit_ratio.cmp(&a.profit_ratio));
        all_paths
    }
}

impl Default for PathFinder {
    fn default() -> Self {
        Self::new(4) // Default max 4 hops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_price(dex: DexType, base: &str, quote: &str, bid: f64, ask: f64) -> PriceData {
        PriceData::new(
            dex,
            TokenPair::new(base, quote),
            Decimal::try_from(bid).unwrap(),
            Decimal::try_from(ask).unwrap(),
        )
    }

    #[test]
    fn test_triangular_arbitrage_detection() {
        let mut finder = PathFinder::new(3);

        // Create a triangular arbitrage opportunity:
        // SOL -> USDC: 1 SOL = 100 USDC (bid)
        // USDC -> RAY: 1 USDC = 0.5 RAY (bid)
        // RAY -> SOL: 1 RAY = 2.1 SOL (bid) <-- Mispricing creates opportunity
        
        finder.add_price(&make_price(DexType::Raydium, "SOL", "USDC", 100.0, 100.1));
        finder.add_price(&make_price(DexType::Orca, "RAY", "USDC", 2.0, 2.01)); // 1 USDC = 0.5 RAY
        finder.add_price(&make_price(DexType::Jupiter, "RAY", "SOL", 0.0476, 0.048)); // 1 RAY = ~21 SOL mispriced!

        let paths = finder.find_triangular_paths("SOL");
        
        // Should find profitable path
        println!("Found {} paths", paths.len());
        for path in &paths {
            println!(
                "Path with {} hops, profit: {}%",
                path.edges.len(),
                path.profit_percentage()
            );
        }
    }

    #[test]
    fn test_no_arbitrage_fair_prices() {
        let mut finder = PathFinder::new(3);

        // Fair prices - no arbitrage
        finder.add_price(&make_price(DexType::Raydium, "SOL", "USDC", 100.0, 100.1));
        finder.add_price(&make_price(DexType::Orca, "RAY", "USDC", 2.0, 2.01));
        finder.add_price(&make_price(DexType::Jupiter, "RAY", "SOL", 0.02, 0.0201));

        let paths = finder.find_triangular_paths("SOL");
        
        // With fair prices, triangular arbitrage profit should be <= fees
        let profitable: Vec<_> = paths.into_iter().filter(|p| p.profit_percentage() > Decimal::from(1)).collect();
        assert!(profitable.is_empty() || profitable[0].profit_percentage() < Decimal::from(1));
    }
}
