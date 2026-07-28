import { invoke } from '@tauri-apps/api/core';
import { useDashboardStore, type DashboardData } from '@/stores/dashboardStore';
import { useComputerStore } from '@/stores/computerStore';
import { useRuntimeStore } from '@/stores/runtimeStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useDashboardStore.setState({
    data: null,
    loading: false,
    error: null,
    requestId: 0,
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
      client_connection_present: true,
      connection_revision: 0,
      connection_context: {
        profile_name: 'prod',
        url: 'https://smcp.example.com',
        office_id: 'office-a',
        computer_name: 'Computer A',
        connected_at: '2026-07-17T00:00:00Z',
        source_type: 'manager_robot',
      },
      connection_profile: 'prod',
      mcp_server_count: 3,
      runtime: runtimeSnapshot({
        lifecycle: 'joined_office',
        mcp_servers: 3,
        active_mcp_servers: 2,
        tools: 10,
      }),
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
    useRuntimeStore.getState().reset();
    useComputerStore.getState().reset();
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchDashboard', () => {
    it('populates dashboard data', async () => {
      mockedInvoke.mockResolvedValueOnce(mockData);

      await useDashboardStore.getState().fetchDashboard();

      expect(mockedInvoke).toHaveBeenCalledWith('get_dashboard_data');
      expect(useDashboardStore.getState().data).toMatchObject(mockData);
      expect(useDashboardStore.getState().data?.computers[0].connection_state).toMatchObject({
        status: 'connected',
        present: true,
        revision: 0,
      });
      expect(useDashboardStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('network error');

      await useDashboardStore.getState().fetchDashboard();

      expect(useDashboardStore.getState().error).toBe('network error');
      expect(useDashboardStore.getState().loading).toBe(false);
    });

    it('restores connection from its own raw authority without ComputerStore', async () => {
      mockedInvoke.mockResolvedValueOnce({
        ...mockData,
        computer_connected: 0,
        computers: [{
          ...mockData.computers[0],
          connected: false,
          connection_profile: undefined,
          runtime: runtimeSnapshot({ lifecycle: 'connected' }),
        }],
      });

      await useDashboardStore.getState().fetchDashboard();
      useRuntimeStore.getState().receiveSnapshot(
        'computer-a',
        runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
      );

      expect(useDashboardStore.getState().data).toMatchObject({
        computer_connected: 1,
        computers: [{ connected: true, connection_profile: 'prod' }],
      });
    });

    it('uses a fresh dashboard authority instead of stale ComputerStore authority', async () => {
      useComputerStore.setState({
        instances: [{
          id: 'computer-a',
          name: 'Stale Computer',
          status: 'running',
          connectionStatus: 'disconnected',
          clientConnectionPresent: false,
          clientConnectionContext: null,
          connectionPolicy: { target: null, auto_connect: false },
          mcpServerCount: 3,
          runtime: runtimeSnapshot({ lifecycle: 'connected' }),
        }],
      });
      mockedInvoke.mockResolvedValueOnce(mockData);

      await useDashboardStore.getState().fetchDashboard();

      expect(useDashboardStore.getState().data).toMatchObject({
        computer_connected: 1,
        computers: [{ connected: true, connection_profile: 'prod' }],
      });
    });

    it('accepts newer connection authority after a higher SDK runtime event', async () => {
      mockedInvoke
        .mockResolvedValueOnce({
          ...mockData,
          computer_connected: 0,
          computers: [{
            ...mockData.computers[0],
            connected: false,
            client_connection_present: false,
            connection_context: null,
            connection_revision: 1,
            connection_profile: undefined,
            runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 1 }),
          }],
        })
        .mockResolvedValueOnce({
          ...mockData,
          computers: [{
            ...mockData.computers[0],
            connection_revision: 2,
            runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
          }],
        });

      await useDashboardStore.getState().fetchDashboard();
      useRuntimeStore.getState().receiveSnapshot(
        'computer-a',
        runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 3 }),
      );
      await useDashboardStore.getState().fetchDashboard();

      expect(useDashboardStore.getState().data).toMatchObject({
        computer_connected: 1,
        computers: [{
          connected: true,
          client_connection_present: true,
          connection_revision: 2,
          connection_profile: 'prod',
        }],
      });
    });

    it('sets loading during fetch', async () => {
      let resolveFn: (v: unknown) => void;
      mockedInvoke.mockImplementationOnce(() => new Promise((r) => { resolveFn = r; }));

      const promise = useDashboardStore.getState().fetchDashboard();
      expect(useDashboardStore.getState().loading).toBe(true);

      resolveFn!(mockData);
      await promise;
      expect(useDashboardStore.getState().loading).toBe(false);
      expect(useDashboardStore.getState().data).toMatchObject(mockData);
      expect(useDashboardStore.getState().data?.computers[0].connection_state).toMatchObject({
        status: 'connected',
        present: true,
        revision: 0,
      });
    });

    it('keeps a newer event snapshot when an older dashboard query finishes later', async () => {
      const currentRuntime = runtimeSnapshot({
        generation: 2,
        snapshot_revision: 2,
        lifecycle: 'joined_office',
        config_revision: 4,
        capability_revision: 5,
      });
      let resolveDashboard!: (value: DashboardData) => void;
      mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => {
        resolveDashboard = resolve;
      }));

      const fetch = useDashboardStore.getState().fetchDashboard();
      useRuntimeStore.getState().receiveSnapshot('computer-a', currentRuntime);
      resolveDashboard({
        ...mockData,
        computers: [{
          ...mockData.computers[0],
          runtime: runtimeSnapshot({
            generation: 2,
            snapshot_revision: 1,
            lifecycle: 'started',
            config_revision: 4,
            capability_revision: 5,
          }),
        }],
      });
      await fetch;

      expect(useDashboardStore.getState().data?.computers[0]).toMatchObject({
        runtime: currentRuntime,
        connected: true,
      });
      expect(useDashboardStore.getState().data?.computer_connected).toBe(1);
    });

    it('filters a deleted Computer from a delayed dashboard response', async () => {
      let resolveDashboard!: (value: DashboardData) => void;
      mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => {
        resolveDashboard = resolve;
      }));

      const fetch = useDashboardStore.getState().fetchDashboard();
      useRuntimeStore.getState().evictSnapshot('computer-a', mockData.computers[0].runtime.incarnation);
      resolveDashboard(mockData);
      await fetch;

      expect(useDashboardStore.getState().data).toMatchObject({
        computer_total: 0,
        computer_running: 0,
        computer_stopped: 0,
        computer_connected: 0,
        computers: [],
      });
    });

    it('ignores an older dashboard request that completes last', async () => {
      let resolveFirst!: (value: DashboardData) => void;
      let resolveSecond!: (value: DashboardData) => void;
      mockedInvoke
        .mockImplementationOnce(() => new Promise((resolve) => {
          resolveFirst = resolve;
        }))
        .mockImplementationOnce(() => new Promise((resolve) => {
          resolveSecond = resolve;
        }));

      const first = useDashboardStore.getState().fetchDashboard();
      const second = useDashboardStore.getState().fetchDashboard();
      resolveSecond({
        ...mockData,
        computers: [{
          ...mockData.computers[0],
          name: 'Newest Computer',
          runtime: runtimeSnapshot({ snapshot_revision: 2, lifecycle: 'joined_office' }),
        }],
      });
      await second;
      resolveFirst(mockData);
      await first;

      expect(useDashboardStore.getState().data?.computers[0].name).toBe('Newest Computer');
      expect(useDashboardStore.getState().requestId).toBe(2);
    });

    it('advances central runtime authority before rejecting a delayed older event', async () => {
      const newest = runtimeSnapshot({
        generation: 2,
        snapshot_revision: 2,
        lifecycle: 'joined_office',
      });
      mockedInvoke.mockResolvedValueOnce({
        ...mockData,
        computers: [{ ...mockData.computers[0], runtime: newest }],
      });

      await useDashboardStore.getState().fetchDashboard();
      useRuntimeStore.getState().receiveSnapshot('computer-a', runtimeSnapshot({
        generation: 2,
        snapshot_revision: 1,
        lifecycle: 'started',
      }));

      expect(useRuntimeStore.getState().snapshots['computer-a']).toEqual(newest);
      expect(useDashboardStore.getState().data?.computers[0]).toMatchObject({
        runtime: newest,
        connected: true,
      });
    });
  });
});
