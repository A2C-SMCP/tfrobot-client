import { invoke } from '@tauri-apps/api/core';
import { useLogStore, type LogEntry } from '@/stores/logStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useLogStore.setState({
    logs: [],
    loading: false,
    error: null,
    filter: { limit: 50, offset: 0 },
  });
}

const mockLog: LogEntry = {
  id: 1,
  timestamp: '2026-01-01T00:00:00Z',
  level: 'info',
  category: 'system',
  message: 'App started',
};

describe('logStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchLogs', () => {
    it('populates logs list', async () => {
      mockedInvoke.mockResolvedValueOnce([mockLog]);

      await useLogStore.getState().fetchLogs();

      expect(mockedInvoke).toHaveBeenCalledWith('get_logs', { filter: { limit: 50, offset: 0 } });
      expect(useLogStore.getState().logs).toEqual([mockLog]);
      expect(useLogStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('db error');

      await useLogStore.getState().fetchLogs();

      expect(useLogStore.getState().error).toBe('db error');
      expect(useLogStore.getState().loading).toBe(false);
    });
  });

  describe('setFilter', () => {
    it('merges partial filter', () => {
      useLogStore.getState().setFilter({ keyword: 'test', levels: ['error'] });

      expect(useLogStore.getState().filter).toEqual({
        limit: 50,
        offset: 0,
        keyword: 'test',
        levels: ['error'],
      });
    });
  });

  describe('exportLogs', () => {
    it('calls invoke with path and filter', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useLogStore.getState().exportLogs('/tmp/logs.json');

      expect(mockedInvoke).toHaveBeenCalledWith('export_logs', {
        path: '/tmp/logs.json',
        filter: { limit: 50, offset: 0 },
      });
    });
  });

  describe('clearLogs', () => {
    it('clears and re-fetches', async () => {
      mockedInvoke.mockResolvedValueOnce(0);    // clear_logs
      mockedInvoke.mockResolvedValueOnce([]);   // fetchLogs

      await useLogStore.getState().clearLogs();

      expect(mockedInvoke).toHaveBeenCalledWith('clear_logs', { beforeDays: null });
    });

    it('passes beforeDays when specified', async () => {
      mockedInvoke.mockResolvedValueOnce(5);
      mockedInvoke.mockResolvedValueOnce([]);

      await useLogStore.getState().clearLogs(7);

      expect(mockedInvoke).toHaveBeenCalledWith('clear_logs', { beforeDays: 7 });
    });
  });
});
