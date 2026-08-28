import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export type ActivityScope =
  | { kind: 'client' }
  | { kind: 'computer'; computer_id: string };

export type ActivityScopeFilter =
  | { kind: 'all' }
  | { kind: 'client_only' }
  | { kind: 'computer'; computer_id: string };

export interface ActivityEvent {
  id: number;
  timestamp: string;
  scope: ActivityScope;
  level: 'debug' | 'info' | 'warn' | 'error';
  category: string;
  event_type: string;
  operation: string;
  outcome: 'succeeded' | 'failed' | 'unknown';
  message: string;
  fields?: unknown;
  correlation_id?: string;
}

export interface ActivityQuery {
  start_time?: string;
  end_time?: string;
  levels?: ActivityEvent['level'][];
  categories?: string[];
  keyword?: string;
  scope: ActivityScopeFilter;
  limit?: number;
  offset?: number;
}

export interface ActivityPage {
  items: ActivityEvent[];
  total: number;
  limit: number;
  offset: number;
}

interface ActivityState {
  items: ActivityEvent[];
  total: number;
  loading: boolean;
  error: string | null;
  query: ActivityQuery;
  requestId: number;
  setQueryAndFetch: (query: Partial<ActivityQuery>) => Promise<void>;
  fetchActivity: () => Promise<void>;
  exportActivity: (path: string) => Promise<void>;
  clearActivity: (scope?: ActivityScopeFilter) => Promise<void>;
  reset: () => void;
}

const initialState = {
  items: [] as ActivityEvent[],
  total: 0,
  loading: false,
  error: null as string | null,
  query: { scope: { kind: 'all' }, limit: 50, offset: 0 } as ActivityQuery,
  requestId: 0,
};

export const useActivityStore = create<ActivityState>((set, get) => {
  const fetchWithQuery = async (query: ActivityQuery) => {
    const requestId = get().requestId + 1;
    set({ query, requestId, loading: true, error: null });
    try {
      const page = await invoke<ActivityPage>('get_activity', { query });
      if (get().requestId === requestId) {
        set({ items: page.items, total: page.total, loading: false });
      }
    } catch (error) {
      if (get().requestId === requestId) {
        set({ error: String(error), loading: false });
      }
    }
  };

  return {
    ...initialState,
    reset: () => set(initialState),
    setQueryAndFetch: async (partial) => {
      await fetchWithQuery({ ...get().query, ...partial });
    },
    fetchActivity: async () => {
      await fetchWithQuery(get().query);
    },
    exportActivity: async (path) => {
      try {
        await invoke('export_activity', { path, query: get().query });
      } catch (error) {
        set({ error: String(error) });
      }
    },
    clearActivity: async (scope) => {
      try {
        await invoke('clear_activity', { scope: scope ?? get().query.scope });
        await fetchWithQuery(get().query);
      } catch (error) {
        set({ error: String(error) });
      }
    },
  };
});
