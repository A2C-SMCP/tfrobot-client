import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import type { LogEntry } from './logStore';

export interface RuntimeInfo {
  name: string;
  path?: string;
  available: boolean;
}

export interface DashboardData {
  connected: boolean;
  connection_url?: string;
  connection_profile?: string;
  mcp_total: number;
  mcp_running: number;
  mcp_stopped: number;
  tools_count: number;
  recent_logs: LogEntry[];
  runtimes: RuntimeInfo[];
}

interface DashboardState {
  data: DashboardData | null;
  loading: boolean;
  error: string | null;

  fetchDashboard: () => Promise<void>;
  reset: () => void;
}

const initialState = {
  data: null as DashboardData | null,
  loading: false,
  error: null as string | null,
};

export const useDashboardStore = create<DashboardState>((set) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchDashboard: async () => {
    set({ loading: true, error: null });
    try {
      const data = await invoke<DashboardData>('get_dashboard_data');
      set({ data, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));
