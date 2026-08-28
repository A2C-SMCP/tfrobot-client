import { invoke } from '@tauri-apps/api/core';
import { useSettingsStore, type AppSettings, type AppInfo, type RuntimeInfo } from '@/stores/settingsStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useSettingsStore.setState({
    settings: null,
    runtimes: [],
    appInfo: null,
    loading: false,
    error: null,
  });
}

const mockSettings: AppSettings = {
  theme: 'system',
  language: 'en',
  diagnostic_log_level: 'info',
  diagnostic_retention_days: 7,
  activity_retention_days: 30,
  tool_history_retention_days: 90,
  custom_runtime_paths: {},
};

const mockRuntimes: RuntimeInfo[] = [
  { name: 'Node.js', path: '/usr/bin/node', version: 'v20.0.0', available: true },
  { name: 'Python', available: false },
];

const mockAppInfo: AppInfo = {
  version: '0.1.0',
  smcp_computer_version: '0.1.0',
};

describe('settingsStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchSettings', () => {
    it('populates settings', async () => {
      mockedInvoke.mockResolvedValueOnce(mockSettings);

      await useSettingsStore.getState().fetchSettings();

      expect(mockedInvoke).toHaveBeenCalledWith('get_settings');
      expect(useSettingsStore.getState().settings).toEqual(mockSettings);
      expect(useSettingsStore.getState().loading).toBe(false);
    });

    it('sets loading during fetch', async () => {
      let resolveFn: (v: unknown) => void;
      mockedInvoke.mockImplementationOnce(() => new Promise((r) => { resolveFn = r; }));

      const promise = useSettingsStore.getState().fetchSettings();
      expect(useSettingsStore.getState().loading).toBe(true);

      resolveFn!(mockSettings);
      await promise;
      expect(useSettingsStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('read error');

      await useSettingsStore.getState().fetchSettings();

      expect(useSettingsStore.getState().error).toBe('read error');
      expect(useSettingsStore.getState().loading).toBe(false);
    });
  });

  describe('updateSettings', () => {
    it('saves and updates local state', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      const updated = { ...mockSettings, language: 'zh' };

      await useSettingsStore.getState().updateSettings(updated);

      expect(mockedInvoke).toHaveBeenCalledWith('update_settings', { settings: updated });
      expect(useSettingsStore.getState().settings).toEqual(updated);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('write error');

      await expect(useSettingsStore.getState().updateSettings(mockSettings)).rejects.toThrow();
      expect(useSettingsStore.getState().error).toBe('write error');
    });
  });

  describe('fetchRuntimes', () => {
    it('populates runtimes', async () => {
      mockedInvoke.mockResolvedValueOnce(mockRuntimes);

      await useSettingsStore.getState().fetchRuntimes();

      expect(mockedInvoke).toHaveBeenCalledWith('detect_runtimes');
      expect(useSettingsStore.getState().runtimes).toEqual(mockRuntimes);
    });
  });

  describe('fetchAppInfo', () => {
    it('populates app info', async () => {
      mockedInvoke.mockResolvedValueOnce(mockAppInfo);

      await useSettingsStore.getState().fetchAppInfo();

      expect(mockedInvoke).toHaveBeenCalledWith('get_app_info');
      expect(useSettingsStore.getState().appInfo).toEqual(mockAppInfo);
    });
  });
});
