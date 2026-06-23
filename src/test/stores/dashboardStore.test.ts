import { invoke } from '@tauri-apps/api/core';
import { useDashboardStore, type DashboardData } from '@/stores/dashboardStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useDashboardStore.setState({
    data: null,
    loading: false,
    error: null,
  });
}

const mockData: DashboardData = {
  computer_total: 1,
  computer_running: 1,
  computer_stopped: 0,
  computer_connected: 1,
  computers: [
    {
      id: 'computer-a',
      name: 'Computer A',
      running: true,
      connected: true,
      mcp_server_count: 3,
    },
  ],
  recent_logs: [],
  runtimes: [
    { name: 'Node.js', path: '/usr/bin/node', available: true },
    { name: 'Python', path: undefined, available: false },
  ],
};

describe('dashboardStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchDashboard', () => {
    it('populates dashboard data', async () => {
      mockedInvoke.mockResolvedValueOnce(mockData);

      await useDashboardStore.getState().fetchDashboard();

      expect(mockedInvoke).toHaveBeenCalledWith('get_dashboard_data');
      expect(useDashboardStore.getState().data).toEqual(mockData);
      expect(useDashboardStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('network error');

      await useDashboardStore.getState().fetchDashboard();

      expect(useDashboardStore.getState().error).toBe('network error');
      expect(useDashboardStore.getState().loading).toBe(false);
    });

    it('sets loading during fetch', async () => {
      let resolveFn: (v: unknown) => void;
      mockedInvoke.mockImplementationOnce(() => new Promise((r) => { resolveFn = r; }));

      const promise = useDashboardStore.getState().fetchDashboard();
      expect(useDashboardStore.getState().loading).toBe(true);

      resolveFn!(mockData);
      await promise;
      expect(useDashboardStore.getState().loading).toBe(false);
      expect(useDashboardStore.getState().data).toEqual(mockData);
    });
  });
});
