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

export interface ActivityState {
  initialized: boolean;
  invalidated: boolean;
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
  initialized: false,
  invalidated: false,
  items: [] as ActivityEvent[],
  total: 0,
  loading: false,
  error: null as string | null,
  query: { scope: { kind: 'all' }, limit: 50, offset: 0 } as ActivityQuery,
  requestId: 0,
};

export function createActivityStore(scope: ActivityScopeFilter = { kind: 'all' }) {
  let serial = 0;
  let lifetime = 0;
  return create<ActivityState>((set, get) => {
  const fetchWithQuery = async (query: ActivityQuery) => {
    const requestId = ++serial;
    set({ query, requestId, loading: true, error: null, invalidated: false });
    try {
      const page = await invoke<ActivityPage>('get_activity', { query });
      if (get().requestId === requestId) {
        set({ items: page.items, total: page.total, loading: false, initialized: true });
      }
    } catch (error) {
      if (get().requestId === requestId) {
        set({ error: String(error), loading: false });
      }
    }
  };

  return {
    ...initialState,
    query: { ...initialState.query, scope },
    reset: () => { lifetime += 1; set({ ...initialState, query: { ...initialState.query, scope }, requestId: ++serial }); },
    setQueryAndFetch: async (partial) => {
      await fetchWithQuery({ ...get().query, ...partial });
    },
    fetchActivity: async () => {
      await fetchWithQuery(get().query);
    },
    exportActivity: async (path) => {
      const started = lifetime;
      try {
        await invoke('export_activity', { path, query: get().query });
      } catch (error) {
        if (lifetime === started) set({ error: String(error) });
      }
    },
    clearActivity: async (scope) => {
      const started = lifetime;
      try {
        await invoke('clear_activity', { scope: scope ?? get().query.scope });
        if (lifetime !== started) return;
        invalidateActivityViews();
        await fetchWithQuery(get().query);
      } catch (error) {
        if (lifetime === started) set({ error: String(error) });
      }
    },
  };
});
}

export const useActivityStore = createActivityStore();
const computerViews = new Map<string, ReturnType<typeof createActivityStore>>();

export function activityViewStore(instanceId?: string) {
  if (!instanceId) return useActivityStore;
  let store = computerViews.get(instanceId);
  if (!store) {
    store = createActivityStore({ kind: 'computer', computer_id: instanceId });
    computerViews.set(instanceId, store);
  }
  return store;
}

function invalidateActivityViews() {
  for (const store of [useActivityStore, ...computerViews.values()]) {
    store.setState({ invalidated: true });
  }
}

export function pruneActivityViews(ids: ReadonlySet<string>) {
  for (const [id, store] of computerViews) {
    if (!ids.has(id)) { store.getState().reset(); computerViews.delete(id); }
  }
}

export function resetActivityViews() {
  useActivityStore.getState().reset();
  for (const store of computerViews.values()) store.getState().reset();
  computerViews.clear();
}
