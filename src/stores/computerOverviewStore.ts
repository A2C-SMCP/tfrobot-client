import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import type { LogEntry } from './logStore';

export interface ComputerOverviewData {
  id: string;
  name: string;
  running: boolean;
  connected: boolean;
  connection_url?: string;
  connection_profile?: string;
  robot_name?: string;
  mcp_total: number;
  mcp_running: number;
  mcp_stopped: number;
  tools_count: number;
  recent_logs: LogEntry[];
}

interface ComputerOverviewState {
  data: ComputerOverviewData | null;
  loading: boolean;
  error: string | null;
  activeInstanceId: string | null;
  requestId: number;
  fetchOverview: (instanceId: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  data: null as ComputerOverviewData | null,
  loading: false,
  error: null as string | null,
  activeInstanceId: null as string | null,
  requestId: 0,
};

export const useComputerOverviewStore = create<ComputerOverviewState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchOverview: async (instanceId: string) => {
    const requestId = get().requestId + 1;
    set({ activeInstanceId: instanceId, requestId, loading: true, error: null });
    try {
      const data = await invoke<ComputerOverviewData>('get_computer_overview_data', { instanceId });
      if (get().requestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ data, loading: false });
    } catch (e) {
      if (get().requestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: String(e), loading: false });
    }
  },
}));
