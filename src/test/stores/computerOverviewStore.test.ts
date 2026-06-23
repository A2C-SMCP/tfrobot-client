import { invoke } from '@tauri-apps/api/core';
import { useComputerOverviewStore, type ComputerOverviewData } from '@/stores/computerOverviewStore';

const mockedInvoke = vi.mocked(invoke);

const mockData: ComputerOverviewData = {
  id: 'computer-a',
  name: 'Computer A',
  running: true,
  connected: true,
  connection_url: 'https://smcp.example.com',
  connection_profile: 'prod',
  robot_name: 'Robot A',
  mcp_total: 3,
  mcp_running: 2,
  mcp_stopped: 1,
  tools_count: 10,
  recent_logs: [],
};

describe('computerOverviewStore', () => {
  beforeEach(() => {
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
});
