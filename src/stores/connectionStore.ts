import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

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

  fetchProfiles: () => Promise<void>;
  fetchStatus: () => Promise<void>;
  saveProfile: (profile: ConnectionProfile, apiKey?: string) => Promise<void>;
  deleteProfile: (name: string) => Promise<void>;
  connect: (profileName: string) => Promise<void>;
  disconnect: () => Promise<void>;
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

  fetchProfiles: async () => {
    set({ loading: true, error: null });
    try {
      const profiles = await invoke<ConnectionProfile[]>('list_profiles');
      set({ profiles, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  fetchStatus: async () => {
    try {
      const status = await invoke<ConnectionStatusInfo>('get_connection_status');
      set({ status });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  saveProfile: async (profile: ConnectionProfile, apiKey?: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('save_profile', { profile, apiKey: apiKey || null });
      await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deleteProfile: async (name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('delete_profile', { name });
      await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  connect: async (profileName: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('connect_smcp', { profileName });
      await get().fetchStatus();
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  reset: () => set(initialState),

  disconnect: async () => {
    set({ loading: true, error: null });
    try {
      await invoke('disconnect_smcp');
      await get().fetchStatus();
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
