import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface DesktopWindow {
  uri: string;
  title: string;
  server: string;
  description?: string;
  mime_type?: string;
}

export interface WindowContent {
  type: 'text' | 'blob';
  uri: string;
  mime_type?: string;
  text?: string;
  blob?: string;
}

export interface WindowDetail {
  uri: string;
  title: string;
  server: string;
  contents: WindowContent[];
}

interface DesktopState {
  windows: DesktopWindow[];
  loading: boolean;
  error: string | null;

  fetchDesktop: (size?: string, uri?: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  windows: [] as DesktopWindow[],
  loading: false,
  error: null as string | null,
};

export const useDesktopStore = create<DesktopState>((set) => ({
  ...initialState,

  reset: () => set(initialState),

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
