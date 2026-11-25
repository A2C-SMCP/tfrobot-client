import type { McpServerManagedBy } from './mcpStore';
import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface ConfigAdoptionChange {
  owner?: McpServerManagedBy | null;
  resource_id: string;
  section: string;
  reason: 'skill_home' | 'startup' | 'mcp_config' | 'disabled_running' | 'input_definition' | 'input_value';
  action: 'start_mcp' | 'restart_mcp' | 'stop_mcp' | 'restart_computer' | 'manage_plugin';
  current_value?: string | null;
  saved_value?: string | null;
}
export interface ConfigAdoptionStatus {
  incarnation: number;
  generation: number;
  state: 'waiting_start' | 'current' | 'pending' | 'unknown';
  changes: ConfigAdoptionChange[];
  unknown_resources: string[];
  plugin_owners?: Record<string, Extract<McpServerManagedBy, { type: 'plugin' }>>;
}
interface State {
  statuses: Record<string, ConfigAdoptionStatus | undefined>;
  errors: Record<string, boolean>;
  loading: Record<string, boolean>;
  watch: (id: string) => () => void;
  invalidate: (id: string) => void;
  recover: () => void;
  reset: () => void;
}
const watchers = new Map<string, number>();
const requests = new Map<string, { again: boolean }>();
let epoch = 0;

export const useConfigAdoptionStore = create<State>((set, get) => ({
  statuses: {}, errors: {}, loading: {},
  watch: (id) => {
    watchers.set(id, (watchers.get(id) ?? 0) + 1);
    get().invalidate(id);
    return () => { const count = (watchers.get(id) ?? 1) - 1; if (count) watchers.set(id, count); else watchers.delete(id); };
  },
  invalidate: (id) => {
    if (!watchers.has(id)) return;
    const pending = requests.get(id);
    if (pending) { pending.again = true; return; }
    const request = { again: false };
    const session = epoch;
    requests.set(id, request);
    set((state) => ({ loading: { ...state.loading, [id]: true } }));
    void (async () => {
      // Repeat only in response to an explicit invalidation received during this request.
      do {
        request.again = false;
        try {
          const status = await invoke<ConfigAdoptionStatus>('get_computer_config_adoption', { id });
          if (epoch !== session || requests.get(id) !== request) return;
          if (!status || !['waiting_start', 'current', 'pending', 'unknown'].includes(status.state) || !Array.isArray(status.changes) || !Array.isArray(status.unknown_resources)) throw new Error('Invalid configuration observation');
          if (!request.again) set((state) => ({ statuses: { ...state.statuses, [id]: status }, errors: { ...state.errors, [id]: false } }));
        } catch {
          if (epoch !== session || requests.get(id) !== request) return;
          set((state) => ({ errors: { ...state.errors, [id]: true } }));
        }
      } while (request.again && watchers.has(id));
      if (requests.get(id) === request) {
        requests.delete(id);
        set((state) => ({ loading: { ...state.loading, [id]: false } }));
      }
    })();
  },
  recover: () => { for (const id of watchers.keys()) get().invalidate(id); },
  reset: () => { epoch += 1; requests.clear(); watchers.clear(); set({ statuses: {}, errors: {}, loading: {} }); },
}));
