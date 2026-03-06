import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

export type ThemeMode = 'light' | 'dark' | 'system';

interface ThemeState {
  mode: ThemeMode;
  resolved: 'light' | 'dark';
  setMode: (mode: ThemeMode) => void;
  initFromSettings: () => Promise<void>;
  reset: () => void;
}

function getSystemTheme(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

const initialState = {
  mode: 'system' as ThemeMode,
  resolved: getSystemTheme() as 'light' | 'dark',
};

export const useThemeStore = create<ThemeState>((set) => ({
  ...initialState,

  reset: () => set(initialState),

  setMode: (mode) => {
    const resolved = mode === 'system' ? getSystemTheme() : mode;
    set({ mode, resolved });
    // Persist to backend settings
    invoke('get_settings').then((settings: unknown) => {
      invoke('update_settings', { settings: { ...(settings as Record<string, unknown>), theme: mode } });
    });
  },

  initFromSettings: async () => {
    try {
      const settings = await invoke<{ theme?: ThemeMode }>('get_settings');
      if (settings.theme) {
        const mode = settings.theme;
        const resolved = mode === 'system' ? getSystemTheme() : mode;
        set({ mode, resolved });
      }
    } catch {
      // Use defaults
    }
  },
}));
