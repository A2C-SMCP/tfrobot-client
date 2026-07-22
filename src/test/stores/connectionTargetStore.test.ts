import { invoke } from '@tauri-apps/api/core';
import { useComputerStore } from '@/stores/computerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import { resetClientConnectionAuthorities } from '@/stores/connectionAuthority';
import {
  useConnectionTargetStore,
  type ManualSmcpTarget,
} from '@/stores/connectionTargetStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

const target: ManualSmcpTarget = {
  id: 'manual-a',
  name: 'prod',
  url: 'https://smcp.example.com',
  namespace: '/smcp',
  office_id: 'office-1',
  headers: { 'X-TF-Namespace': 'ns' },
};

function resetStores() {
  useConnectionTargetStore.setState({
    manualTargets: [],
    loading: false,
    error: null,
  });
  useConnectionStore.setState({
    statuses: {},
    loading: false,
    error: null,
  });
  useComputerStore.setState({
    instances: [],
    loading: false,
    error: null,
    selectedInstanceId: null,
    listRequestId: 0,
    mutationEpoch: 0,
    profileMutationVersions: {},
    connectionMetadataRequestIds: {},
  });
  resetClientConnectionAuthorities();
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

    const saved = await useConnectionTargetStore
      .getState()
      .saveManualTarget(target, { kind: 'set', value: 'secret' });

    expect(saved).toEqual(target);
    expect(mockedInvoke).toHaveBeenCalledWith('save_manual_smcp_target', {
      target,
      apiKeyAction: { kind: 'set', value: 'secret' },
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

  it('connectTarget delegates status projection to runtime events', async () => {
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'Computer A',
        status: 'running',
        connectionStatus: 'disconnected',
        connectionPolicy: { target: null, auto_connect: false },
        mcpServerCount: 0,
        runtime: runtimeSnapshot({ lifecycle: 'started' }),
      }],
    });
    mockedInvoke.mockResolvedValueOnce(undefined);
    mockedInvoke.mockResolvedValueOnce([
      {
        id: 'computer-a',
        name: 'Computer A',
        running: true,
        runtime: runtimeSnapshot({ lifecycle: 'joined_office' }),
        connected: true,
        client_connection_present: true,
        connection_revision: 7,
        connection_context: {
          profile_name: 'prod',
          url: 'https://smcp.example.com',
          office_id: 'office-1',
          computer_name: 'Computer A',
          connected_at: '2026-07-17T00:00:00Z',
          source_type: 'manual_smcp',
          target_id: 'manual-a',
        },
        connection_policy: {
          target: { type: 'manual_smcp', id: 'manual-a' },
          auto_connect: false,
        },
        mcp_server_count: 0,
      },
    ]);

    await useConnectionTargetStore.getState().connectTarget('computer-a', 'manual-a');

    expect(mockedInvoke).toHaveBeenCalledWith('connect_connection_target', {
      instanceId: 'computer-a',
      targetId: 'manual-a',
    });
    expect(mockedInvoke).toHaveBeenCalledWith('list_computer_instances');
    expect(mockedInvoke).not.toHaveBeenCalledWith('get_connection_status', expect.anything());
    expect(useConnectionStore.getState().getStatus('computer-a')).toEqual({ connected: false });
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      connectionStatus: 'disconnected',
      connectionPolicy: {
        target: { type: 'manual_smcp', id: 'manual-a' },
        auto_connect: false,
      },
    });
  });

  it('stores and rethrows backend errors', async () => {
    mockedInvoke.mockRejectedValueOnce('connect failed');

    await expect(
      useConnectionTargetStore.getState().connectTarget('computer-a', 'manual-a'),
    ).rejects.toBe('connect failed');

    expect(useConnectionTargetStore.getState().error).toBe('connect failed');
    expect(useConnectionTargetStore.getState().loading).toBe(false);
  });

  it('surfaces a partial-success warning when post-connect metadata refresh fails', async () => {
    mockedInvoke
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce('metadata unavailable');

    await expect(
      useConnectionTargetStore.getState().connectTarget('computer-a', 'manual-a'),
    ).rejects.toThrow(
      'Connection succeeded, but refreshing its saved binding and policy failed: metadata unavailable',
    );

    expect(useConnectionTargetStore.getState()).toMatchObject({
      loading: false,
      error: 'Error: Connection succeeded, but refreshing its saved binding and policy failed: metadata unavailable',
    });
  });
});
