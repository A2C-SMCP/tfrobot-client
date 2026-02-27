import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface DesktopWindow {
  uri: string;
  title: string;
  server: string;
  description?: string;
  mime_type?: string;
}

interface DesktopState {
  windows: DesktopWindow[];
  loading: boolean;
  error: string | null;

  fetchDesktop: (size?: string, uri?: string) => Promise<void>;
}

export const useDesktopStore = create<DesktopState>((set) => ({
  windows: [],
  loading: false,
  error: null,

  fetchDesktop: async (size?: string, uri?: string) => {
    set({ loading: true, error: null });
    try {
      const windows = await invoke<DesktopWindow[]>('get_desktop', {
        size: size ?? null,
        uri: uri ?? null,
      });
      set({ windows, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));
