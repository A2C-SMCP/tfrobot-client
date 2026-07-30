import { invoke } from '@tauri-apps/api/core';
import { useComputerOverviewStore, type ComputerOverviewData } from '@/stores/computerOverviewStore';
import { useComputerStore } from '@/stores/computerStore';
import { useRuntimeStore } from '@/stores/runtimeStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

const mockData: ComputerOverviewData = {
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
  connection_url: 'https://smcp.example.com',
  connection_profile: 'prod',
  robot_name: 'Robot A',
  mcp_total: 3,
  mcp_running: 2,
  mcp_stopped: 1,
  tools_count: 10,
  runtime: runtimeSnapshot({
    lifecycle: 'joined_office',
    mcp_servers: 3,
    active_mcp_servers: 2,
    tools: 10,
  }),
  recent_logs: [],
};

describe('computerOverviewStore', () => {
  beforeEach(() => {
    useRuntimeStore.getState().reset();
    useComputerStore.getState().reset();
    useComputerOverviewStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('fetches overview data for a computer instance', async () => {
    mockedInvoke.mockResolvedValueOnce(mockData);

    await useComputerOverviewStore.getState().fetchOverview('computer-a');

    expect(mockedInvoke).toHaveBeenCalledWith('get_computer_overview_data', { instanceId: 'computer-a' });
    expect(useComputerOverviewStore.getState().data).toEqual(mockData);
    expect(useComputerOverviewStore.getState().loading).toBe(false);
  });

  it('ignores stale responses from a previous computer instance', async () => {
    let resolveFirst: (value: ComputerOverviewData) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce({ ...mockData, id: 'computer-b', name: 'Computer B' });

    const first = useComputerOverviewStore.getState().fetchOverview('computer-a');
    const second = useComputerOverviewStore.getState().fetchOverview('computer-b');

    resolveFirst!(mockData);
    await Promise.all([first, second]);

    expect(useComputerOverviewStore.getState().data?.id).toBe('computer-b');
  });

  it('sets error on failure', async () => {
    mockedInvoke.mockRejectedValueOnce('network error');

    await useComputerOverviewStore.getState().fetchOverview('computer-a');

    expect(useComputerOverviewStore.getState().error).toBe('network error');
    expect(useComputerOverviewStore.getState().loading).toBe(false);
  });

  it('restores connection from its own raw authority without ComputerStore', async () => {
    mockedInvoke.mockResolvedValueOnce({
      ...mockData,
      connected: false,
      connection_url: undefined,
      connection_profile: undefined,
      runtime: runtimeSnapshot({ lifecycle: 'connected' }),
    });

    await useComputerOverviewStore.getState().fetchOverview('computer-a');
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
    );

    expect(useComputerOverviewStore.getState().data).toMatchObject({
      connected: true,
      connection_profile: 'prod',
      connection_url: 'https://smcp.example.com',
    });
  });

  it('uses a fresh overview authority instead of stale ComputerStore authority', async () => {
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

    await useComputerOverviewStore.getState().fetchOverview('computer-a');

    expect(useComputerOverviewStore.getState().data).toMatchObject({
      connected: true,
      connection_profile: 'prod',
      connection_url: 'https://smcp.example.com',
    });
  });

  it('accepts newer connection authority after a higher SDK runtime event', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        ...mockData,
        connected: false,
        client_connection_present: false,
        connection_context: null,
        connection_revision: 1,
        connection_url: undefined,
        connection_profile: undefined,
        runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 1 }),
      })
      .mockResolvedValueOnce({
        ...mockData,
        connection_revision: 2,
        runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
      });

    await useComputerOverviewStore.getState().fetchOverview('computer-a');
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 3 }),
    );
    await useComputerOverviewStore.getState().fetchOverview('computer-a');

    expect(useComputerOverviewStore.getState().data).toMatchObject({
      connected: true,
      client_connection_present: true,
      connection_revision: 2,
      connection_profile: 'prod',
      connection_url: 'https://smcp.example.com',
    });
  });

  it('keeps a newer event snapshot when an older overview query finishes later', async () => {
    const currentRuntime = runtimeSnapshot({
      generation: 2,
      snapshot_revision: 2,
      lifecycle: 'joined_office',
      config_revision: 4,
      capability_revision: 5,
    });
    let resolveOverview!: (value: ComputerOverviewData) => void;
    mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => {
      resolveOverview = resolve;
    }));

    const fetch = useComputerOverviewStore.getState().fetchOverview('computer-a');
    useRuntimeStore.getState().receiveSnapshot('computer-a', currentRuntime);
    resolveOverview({
      ...mockData,
      runtime: runtimeSnapshot({
        generation: 2,
        snapshot_revision: 1,
        lifecycle: 'started',
        config_revision: 4,
        capability_revision: 5,
      }),
    });
    await fetch;

    expect(useComputerOverviewStore.getState().data).toMatchObject({
      runtime: currentRuntime,
      connected: true,
    });
  });
});
