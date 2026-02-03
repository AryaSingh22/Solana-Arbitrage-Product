// API Client for the Solana Arbitrage Backend

import type { ApiResponse, ArbitrageOpportunity, PriceData, Config, StatusData } from './types';

const API_BASE = import.meta.env.VITE_API_URL || 'http://localhost:8080';

async function fetchApi<T>(endpoint: string): Promise<ApiResponse<T>> {
    try {
        const response = await fetch(`${API_BASE}${endpoint}`);
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }
        return await response.json();
    } catch (error) {
        return {
            success: false,
            error: error instanceof Error ? error.message : 'Unknown error',
        };
    }
}

export const api = {
    // Health check
    async health(): Promise<ApiResponse<{ status: string; version: string }>> {
        return fetchApi('/health');
    },

    // Get arbitrage opportunities
    async getOpportunities(params?: {
        minProfit?: number;
        limit?: number;
    }): Promise<ApiResponse<ArbitrageOpportunity[]>> {
        const query = new URLSearchParams();
        if (params?.minProfit) query.set('min_profit', params.minProfit.toString());
        if (params?.limit) query.set('limit', params.limit.toString());
        const queryStr = query.toString();
        return fetchApi(`/api/opportunities${queryStr ? `?${queryStr}` : ''}`);
    },

    // Get single opportunity
    async getOpportunity(id: string): Promise<ApiResponse<ArbitrageOpportunity>> {
        return fetchApi(`/api/opportunities/${id}`);
    },

    // Get prices
    async getPrices(params?: {
        base?: string;
        quote?: string;
    }): Promise<ApiResponse<PriceData[]>> {
        const query = new URLSearchParams();
        if (params?.base) query.set('base', params.base);
        if (params?.quote) query.set('quote', params.quote);
        const queryStr = query.toString();
        return fetchApi(`/api/prices${queryStr ? `?${queryStr}` : ''}`);
    },

    // Get pair prices
    async getPairPrices(pair: string): Promise<ApiResponse<PriceData[]>> {
        return fetchApi(`/api/prices/${pair}`);
    },

    // Get config
    async getConfig(): Promise<ApiResponse<Config>> {
        return fetchApi('/api/config');
    },

    // Get bot status
    async getStatus(): Promise<ApiResponse<StatusData>> {
        return fetchApi('/api/status');
    },
};
