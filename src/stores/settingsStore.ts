import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface RuntimeInfo {
  name: string;
  path?: string;
  version?: string;
  available: boolean;
}

export interface CustomRuntimePaths {
  node?: string;
  python?: string;
  uv?: string;
  pnpm?: string;
}

export interface AppSettings {
  theme: string;
  language: string;
  log_retention_days: number;
  custom_runtime_paths: CustomRuntimePaths;
}

export interface AppInfo {
  version: string;
  smcp_computer_version: string;
}

interface SettingsState {
  settings: AppSettings | null;
  runtimes: RuntimeInfo[];
  appInfo: AppInfo | null;
  loading: boolean;
  error: string | null;

  fetchSettings: () => Promise<void>;
  updateSettings: (settings: AppSettings) => Promise<void>;
  fetchRuntimes: () => Promise<void>;
  fetchAppInfo: () => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  settings: null,
  runtimes: [],
  appInfo: null,
  loading: false,
  error: null,

  fetchSettings: async () => {
    set({ loading: true, error: null });
    try {
      const settings = await invoke<AppSettings>('get_settings');
      set({ settings, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  updateSettings: async (settings: AppSettings) => {
    set({ loading: true, error: null });
    try {
      await invoke('update_settings', { settings });
      set({ settings, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  fetchRuntimes: async () => {
    try {
      const runtimes = await invoke<RuntimeInfo[]>('detect_runtimes');
      set({ runtimes });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchAppInfo: async () => {
    try {
      const appInfo = await invoke<AppInfo>('get_app_info');
      set({ appInfo });
    } catch (e) {
      set({ error: String(e) });
    }
  },
}));
