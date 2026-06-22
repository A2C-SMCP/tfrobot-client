import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info } from '@/utils/logger';

export interface ConnectionProfile {
  name: string;
  url: string;
  namespace: string;
  office_id: string;
  computer_name: string;
  api_key_ref?: string;
  headers: Record<string, string>;
  auto_connect: boolean;
  auto_reconnect: boolean;
}

export interface ConnectionStatusInfo {
  connected: boolean;
  url?: string;
  office_id?: string;
  computer_name?: string;
  connected_at?: string;
  profile_name?: string;
}

interface ConnectionState {
  profiles: ConnectionProfile[];
  status: ConnectionStatusInfo;
  loading: boolean;
  error: string | null;

  fetchProfiles: (instanceId: string) => Promise<void>;
  fetchStatus: (instanceId: string) => Promise<void>;
  saveProfile: (instanceId: string, profile: ConnectionProfile, apiKey?: string) => Promise<void>;
  deleteProfile: (instanceId: string, name: string) => Promise<void>;
  connect: (instanceId: string, profileName: string) => Promise<void>;
  disconnect: (instanceId: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  profiles: [] as ConnectionProfile[],
  status: { connected: false } as ConnectionStatusInfo,
  loading: false,
  error: null as string | null,
};

export const useConnectionStore = create<ConnectionState>((set, get) => ({
  ...initialState,

  fetchProfiles: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      const profiles = await invoke<ConnectionProfile[]>('list_profiles', { instanceId });
      set({ profiles, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  fetchStatus: async (instanceId: string) => {
    try {
      const status = await invoke<ConnectionStatusInfo>('get_connection_status', { instanceId });
      set({ status });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  saveProfile: async (instanceId: string, profile: ConnectionProfile, apiKey?: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('save_profile', { instanceId, profile, apiKey: apiKey || null });
      await get().fetchProfiles(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deleteProfile: async (instanceId: string, name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('delete_profile', { instanceId, name });
      await get().fetchProfiles(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  connect: async (instanceId: string, profileName: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('connect_smcp', { instanceId, profileName });
      info(`SMCP connected: ${profileName}`);
      await get().fetchStatus(instanceId);
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  reset: () => set(initialState),

  disconnect: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('disconnect_smcp', { instanceId });
      info('SMCP disconnected');
      await get().fetchStatus(instanceId);
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
