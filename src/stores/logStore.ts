import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface LogEntry {
  id: number;
  timestamp: string;
  level: string;
  category: string;
  message: string;
  details?: string;
  computer_instance_id?: string;
}

export interface LogFilter {
  start_time?: string;
  end_time?: string;
  levels?: string[];
  categories?: string[];
  keyword?: string;
  computer_instance_id?: string;
  limit?: number;
  offset?: number;
}

interface LogState {
  logs: LogEntry[];
  loading: boolean;
  error: string | null;
  filter: LogFilter;
  logsRequestId: number;

  setFilter: (filter: Partial<LogFilter>) => void;
  setFilterAndFetch: (filter: Partial<LogFilter>) => Promise<void>;
  fetchLogs: () => Promise<void>;
  exportLogs: (path: string) => Promise<void>;
  clearLogs: (beforeDays?: number) => Promise<void>;
  reset: () => void;
}

const initialState = {
  logs: [] as LogEntry[],
  loading: false,
  error: null as string | null,
  filter: { limit: 50, offset: 0 } as LogFilter,
  logsRequestId: 0,
};

export const useLogStore = create<LogState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  setFilter: (partial) => {
    set((s) => ({ filter: { ...s.filter, ...partial } }));
  },

  setFilterAndFetch: async (partial) => {
    const nextFilter = { ...get().filter, ...partial };
    const requestId = get().logsRequestId + 1;
    set({ filter: nextFilter, logsRequestId: requestId, loading: true, error: null });
    try {
      const logs = await invoke<LogEntry[]>('get_logs', { filter: nextFilter });
      if (get().logsRequestId !== requestId) {
        return;
      }
      set({ logs, loading: false });
    } catch (e) {
      if (get().logsRequestId !== requestId) {
        return;
      }
      set({ error: String(e), loading: false });
    }
  },

  fetchLogs: async () => {
    const requestId = get().logsRequestId + 1;
    const filter = get().filter;
    set({ logsRequestId: requestId, loading: true, error: null });
    try {
      const logs = await invoke<LogEntry[]>('get_logs', { filter });
      if (get().logsRequestId !== requestId) {
        return;
      }
      set({ logs, loading: false });
    } catch (e) {
      if (get().logsRequestId !== requestId) {
        return;
      }
      set({ error: String(e), loading: false });
    }
  },

  exportLogs: async (path: string) => {
    try {
      await invoke('export_logs', { path, filter: get().filter });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  clearLogs: async (beforeDays?: number) => {
    try {
      await invoke('clear_logs', { beforeDays: beforeDays ?? null });
      await get().fetchLogs();
    } catch (e) {
      set({ error: String(e) });
    }
  },
}));
