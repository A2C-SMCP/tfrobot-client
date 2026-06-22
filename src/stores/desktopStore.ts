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
  loadingDetails: Record<string, boolean>;
  detailErrors: Record<string, string>;

  fetchDesktop: (instanceId: string, uri?: string) => Promise<void>;
  fetchWindowDetail: (instanceId: string, server: string, uri: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  windows: [] as DesktopWindow[],
  loading: false,
  error: null as string | null,
  windowDetails: {} as Record<string, WindowDetail>,
  loadingDetails: {} as Record<string, boolean>,
  detailErrors: {} as Record<string, string>,
};

export const useDesktopStore = create<DesktopState>((set) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchDesktop: async (instanceId: string, uri?: string) => {
    set({ loading: true, error: null });
    try {
      const windows = await invoke<DesktopWindow[]>('get_desktop', {
        instanceId,
        uri: uri ?? null,
      });
      set({ windows, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  fetchWindowDetail: async (instanceId: string, server: string, uri: string) => {
    // Add to loading record
    set((state) => ({
      loadingDetails: { ...state.loadingDetails, [uri]: true },
    }));

    try {
      const detail = await invoke<WindowDetail>('get_window_detail', {
        instanceId,
        serverName: server,
        uri,
      });
      set((state) => {
        const { [uri]: _, ...remainingLoading } = state.loadingDetails;
        const { [uri]: __, ...remainingErrors } = state.detailErrors;
        return {
          windowDetails: { ...state.windowDetails, [uri]: detail },
          loadingDetails: remainingLoading,
          detailErrors: remainingErrors,
        };
      });
    } catch (e) {
      set((state) => {
        const { [uri]: _, ...remainingLoading } = state.loadingDetails;
        return {
          loadingDetails: remainingLoading,
          detailErrors: { ...state.detailErrors, [uri]: String(e) },
        };
      });
    }
  },
}));
