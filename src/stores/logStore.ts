import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface LogEntry {
  id: number;
  timestamp: string;
  level: string;
  category: string;
  message: string;
  details?: string;
}

export interface LogFilter {
  start_time?: string;
  end_time?: string;
  levels?: string[];
  categories?: string[];
  keyword?: string;
  limit?: number;
  offset?: number;
}

interface LogState {
  logs: LogEntry[];
  loading: boolean;
  error: string | null;
  filter: LogFilter;

  setFilter: (filter: Partial<LogFilter>) => void;
  fetchLogs: () => Promise<void>;
  exportLogs: (path: string) => Promise<void>;
  clearLogs: (beforeDays?: number) => Promise<void>;
}

export const useLogStore = create<LogState>((set, get) => ({
  logs: [],
  loading: false,
  error: null,
  filter: { limit: 50, offset: 0 },

  setFilter: (partial) => {
    set((s) => ({ filter: { ...s.filter, ...partial } }));
  },

  fetchLogs: async () => {
    set({ loading: true, error: null });
    try {
      const logs = await invoke<LogEntry[]>('get_logs', { filter: get().filter });
      set({ logs, loading: false });
    } catch (e) {
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
