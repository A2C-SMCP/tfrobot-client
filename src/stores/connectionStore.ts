import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info } from '@/utils/logger';
import { useComputerStore } from './computerStore';

export interface ConnectionStatusInfo {
  connected: boolean;
  url?: string;
  office_id?: string;
  computer_name?: string;
  connected_at?: string;
  profile_name?: string;
  source_type?: 'manual_smcp' | 'manager_robot';
  target_id?: string;
  target_name?: string;
  employee_id?: number;
}

interface ConnectionState {
  statuses: Record<string, ConnectionStatusInfo>;
  loading: boolean;
  error: string | null;

  getStatus: (instanceId: string) => ConnectionStatusInfo;
  fetchStatus: (instanceId: string) => Promise<void>;
  disconnect: (instanceId: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  statuses: {} as Record<string, ConnectionStatusInfo>,
  loading: false,
  error: null as string | null,
};

export const useConnectionStore = create<ConnectionState>((set, get) => ({
  ...initialState,

  getStatus: (instanceId: string) => get().statuses[instanceId] ?? { connected: false },

  fetchStatus: async (instanceId: string) => {
    try {
      const status = await invoke<ConnectionStatusInfo>('get_connection_status', { instanceId });
      set((state) => ({
        statuses: {
          ...state.statuses,
          [instanceId]: status,
        },
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  reset: () => set(initialState),

  disconnect: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('disconnect_smcp', { instanceId });
      info('SMCP disconnected');
      await get().fetchStatus(instanceId);
      await useComputerStore.getState().fetchInstances();
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
