import { invoke } from '@tauri-apps/api/core';
import { useThemeStore } from '@/stores/themeStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useThemeStore.setState({
    mode: 'system',
    resolved: 'light',
  });
}

describe('themeStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('setMode', () => {
    it('sets light mode', () => {
      // Mock get_settings and update_settings for persistence
      mockedInvoke.mockResolvedValueOnce({ theme: 'light' });
      mockedInvoke.mockResolvedValueOnce(undefined);

      useThemeStore.getState().setMode('light');

      expect(useThemeStore.getState().mode).toBe('light');
      expect(useThemeStore.getState().resolved).toBe('light');
    });

    it('sets dark mode', () => {
      mockedInvoke.mockResolvedValueOnce({ theme: 'dark' });
      mockedInvoke.mockResolvedValueOnce(undefined);

      useThemeStore.getState().setMode('dark');

      expect(useThemeStore.getState().mode).toBe('dark');
      expect(useThemeStore.getState().resolved).toBe('dark');
    });

    it('sets system mode resolves to light when matchMedia is false', () => {
      mockedInvoke.mockResolvedValueOnce({ theme: 'system' });
      mockedInvoke.mockResolvedValueOnce(undefined);

      useThemeStore.getState().setMode('system');

      expect(useThemeStore.getState().mode).toBe('system');
      // matchMedia is mocked to return false in test setup
      expect(useThemeStore.getState().resolved).toBe('light');
    });
  });

  describe('initFromSettings', () => {
    it('loads theme from backend settings', async () => {
      mockedInvoke.mockResolvedValueOnce({ theme: 'dark' });

      await useThemeStore.getState().initFromSettings();

      expect(useThemeStore.getState().mode).toBe('dark');
      expect(useThemeStore.getState().resolved).toBe('dark');
    });

    it('keeps defaults on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('not available');

      await useThemeStore.getState().initFromSettings();

      expect(useThemeStore.getState().mode).toBe('system');
    });
  });
});
