import { invoke } from '@tauri-apps/api/core';
import { useComputerStore } from '@/stores/computerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import {
  useConnectionTargetStore,
  type ManualSmcpTarget,
} from '@/stores/connectionTargetStore';

const mockedInvoke = vi.mocked(invoke);

const target: ManualSmcpTarget = {
  id: 'manual-a',
  name: 'prod',
  url: 'https://smcp.example.com',
  namespace: '/smcp',
  office_id: 'office-1',
  computer_name: 'my-pc',
  headers: { 'X-TF-Namespace': 'ns' },
  auto_connect: true,
  auto_reconnect: true,
};

function resetStores() {
  useConnectionTargetStore.setState({
    manualTargets: [],
    loading: false,
    error: null,
  });
  useConnectionStore.setState({
    profiles: [],
    statuses: {},
    loading: false,
    error: null,
  });
  useComputerStore.setState({
    instances: [],
    loading: false,
    error: null,
    selectedInstanceId: null,
  });
}

describe('connectionTargetStore', () => {
  beforeEach(() => {
    resetStores();
    mockedInvoke.mockReset();
  });

  it('fetchManualTargets populates manual targets', async () => {
    mockedInvoke.mockResolvedValueOnce([target]);

    await useConnectionTargetStore.getState().fetchManualTargets();

    expect(mockedInvoke).toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(useConnectionTargetStore.getState().manualTargets).toEqual([target]);
    expect(useConnectionTargetStore.getState().loading).toBe(false);
  });

  it('saveManualTarget invokes backend with api key and refreshes targets', async () => {
    mockedInvoke.mockResolvedValueOnce(target);
    mockedInvoke.mockResolvedValueOnce([target]);

    const saved = await useConnectionTargetStore.getState().saveManualTarget(target, 'secret');

    expect(saved).toEqual(target);
    expect(mockedInvoke).toHaveBeenCalledWith('save_manual_smcp_target', {
      target,
      apiKey: 'secret',
    });
    expect(mockedInvoke).toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(useConnectionTargetStore.getState().manualTargets).toEqual([target]);
  });

  it('deleteManualTarget invokes backend and refreshes targets', async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    mockedInvoke.mockResolvedValueOnce([]);

    await useConnectionTargetStore.getState().deleteManualTarget('manual-a');

    expect(mockedInvoke).toHaveBeenCalledWith('delete_manual_smcp_target', {
      targetId: 'manual-a',
    });
    expect(mockedInvoke).toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(useConnectionTargetStore.getState().manualTargets).toEqual([]);
  });

  it('connectTarget refreshes connection status and computer instances', async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    mockedInvoke.mockResolvedValueOnce({
      connected: true,
      target_id: 'manual-a',
    });
    mockedInvoke.mockResolvedValueOnce([
      {
        id: 'computer-a',
        name: 'Computer A',
        running: true,
        connected: true,
        mcp_server_count: 0,
      },
    ]);

    await useConnectionTargetStore.getState().connectTarget('computer-a', 'manual-a');

    expect(mockedInvoke).toHaveBeenCalledWith('connect_connection_target', {
      instanceId: 'computer-a',
      targetId: 'manual-a',
    });
    expect(mockedInvoke).toHaveBeenCalledWith('get_connection_status', {
      instanceId: 'computer-a',
    });
    expect(mockedInvoke).toHaveBeenCalledWith('list_computer_instances');
    expect(useConnectionStore.getState().getStatus('computer-a')).toEqual({
      connected: true,
      target_id: 'manual-a',
    });
    expect(useComputerStore.getState().instances).toHaveLength(1);
  });

  it('stores and rethrows backend errors', async () => {
    mockedInvoke.mockRejectedValueOnce('connect failed');

    await expect(
      useConnectionTargetStore.getState().connectTarget('computer-a', 'manual-a'),
    ).rejects.toBe('connect failed');

    expect(useConnectionTargetStore.getState().error).toBe('connect failed');
    expect(useConnectionTargetStore.getState().loading).toBe(false);
  });
});
