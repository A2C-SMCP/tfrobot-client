import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import type { LogEntry } from './logStore';

export interface RuntimeInfo {
  name: string;
  path?: string;
  available: boolean;
}

export interface DashboardComputerSummary {
  id: string;
  name: string;
  running: boolean;
  connected: boolean;
  mcp_server_count: number;
  robot_name?: string;
  connection_profile?: string;
}

export interface DashboardData {
  computer_total: number;
  computer_running: number;
  computer_stopped: number;
  computer_connected: number;
  computers: DashboardComputerSummary[];
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
