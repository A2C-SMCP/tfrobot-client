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
  title?: string;
  server: string;
  contents: WindowContent[];
}

interface DesktopState {
  windows: DesktopWindow[];
  loading: boolean;
  error: string | null;

  // Window detail states
  windowDetails: Record<string, WindowDetail>;
  loadingDetails: Set<string>;
  detailErrors: Record<string, string>;

  fetchDesktop: (size?: string, uri?: string) => Promise<void>;
  fetchWindowDetail: (server: string, uri: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  windows: [] as DesktopWindow[],
  loading: false,
  error: null as string | null,
  windowDetails: {} as Record<string, WindowDetail>,
  loadingDetails: new Set<string>(),
  detailErrors: {} as Record<string, string>,
};

export const useDesktopStore = create<DesktopState>((set, get) => ({
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

  fetchWindowDetail: async (server: string, uri: string) => {
    // Add to loading set
    set((state) => ({
      loadingDetails: new Set(state.loadingDetails).add(uri),
    }));

    // Clear previous error for this uri
    const { detailErrors } = get();
    const newErrors = { ...detailErrors };
    delete newErrors[uri];

    try {
      const detail = await invoke<WindowDetail>('get_window_detail', {
        serverName: server,
        uri,
      });
      set((state) => ({
        windowDetails: { ...state.windowDetails, [uri]: detail },
        loadingDetails: (() => {
          const next = new Set(state.loadingDetails);
          next.delete(uri);
          return next;
        })(),
        detailErrors: newErrors,
      }));
    } catch (e) {
      set((state) => ({
        loadingDetails: (() => {
          const next = new Set(state.loadingDetails);
          next.delete(uri);
          return next;
        })(),
        detailErrors: { ...state.detailErrors, [uri]: String(e) },
      }));
    }
  },
}));
