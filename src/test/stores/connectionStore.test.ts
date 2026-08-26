import { invoke } from '@tauri-apps/api/core';
import { debug, error as logError } from '@tauri-apps/plugin-log';
import { useConnectionStore } from '@/stores/connectionStore';
import {
  resetClientConnectionAuthorities,
  setClientConnectionAuthority,
} from '@/stores/connectionAuthority';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);
const mockedDebug = vi.mocked(debug);
const mockedLogError = vi.mocked(logError);
const instanceId = 'computer-a';

function resetStore() {
  useConnectionStore.setState({
    statuses: {},
    loading: false,
    error: null,
  });
}

describe('connectionStore', () => {
  beforeEach(() => {
    resetStore();
    resetClientConnectionAuthorities();
    mockedInvoke.mockReset();
    mockedDebug.mockReset();
    mockedLogError.mockReset();
  });

  it('projects the canonical four-state connection snapshot independently of SDK lifecycle', () => {
    const joined = runtimeSnapshot({ lifecycle: 'joined_office' });
    setClientConnectionAuthority(instanceId, true, {
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
      profile_name: 'prod',
      source_type: 'manual_smcp',
      target_id: 'target-a',
    }, 1, joined);

    useConnectionStore.getState().applyRuntimeSnapshot(instanceId, joined);
    expect(useConnectionStore.getState().getStatus(instanceId)).toMatchObject({
      status: 'connected',
      connected: true,
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
      profile_name: 'prod',
      source_type: 'manual_smcp',
      target_id: 'target-a',
    });

    useConnectionStore.getState().applyRuntimeSnapshot(
      instanceId,
      runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
    );
    expect(useConnectionStore.getState().getStatus(instanceId)).toMatchObject({
      status: 'connected',
      connected: true,
    });

    setClientConnectionAuthority(instanceId, {
      status: 'connecting',
      present: true,
      revision: 2,
      context: {
        url: 'https://smcp.example.com',
        office_id: 'office-a',
        computer_name: 'Computer A',
        connected_at: '2026-07-17T00:00:00Z',
        profile_name: 'prod',
      },
      operation: 'reconnect',
      operation_target: {
        source_type: 'manual_smcp',
        target_id: 'target-a',
        employee_id: null,
      },
      last_error: {
        operation: 'reconnect',
        message: 'retry 1/3',
        retryable: true,
        occurred_at: '2026-07-29T02:00:00Z',
      },
      actions: {
        connect: { enabled: false, disabled_reason: 'transition_in_progress' },
        disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
      },
    }, runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }));
    useConnectionStore.getState().applyRuntimeSnapshot(
      instanceId,
      runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
    );
    expect(useConnectionStore.getState().getStatus(instanceId)).toMatchObject({
      status: 'connecting',
      connected: false,
      operation: 'reconnect',
      operation_target: {
        source_type: 'manual_smcp',
        target_id: 'target-a',
      },
      profile_name: 'prod',
      last_error: { message: 'retry 1/3', retryable: true },
    });
  });

  it('restores a pending Manager target without frontend selection state', () => {
    const runtime = runtimeSnapshot({ lifecycle: 'started' });
    setClientConnectionAuthority(instanceId, {
      status: 'connecting',
      present: false,
      revision: 1,
      context: null,
      operation: 'connect',
      operation_target: {
        source_type: 'manager_robot',
        target_id: 'manager:11',
        employee_id: 11,
      },
      last_error: null,
      actions: {
        connect: { enabled: false, disabled_reason: 'transition_in_progress' },
        disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
      },
    }, runtime);

    useConnectionStore.getState().applyRuntimeSnapshot(instanceId, runtime);

    expect(useConnectionStore.getState().getStatus(instanceId)).toMatchObject({
      status: 'connecting',
      connected: false,
      operation_target: {
        source_type: 'manager_robot',
        target_id: 'manager:11',
        employee_id: 11,
      },
    });
  });

  describe('disconnect', () => {
    it('invokes disconnect_smcp and lets runtime events update status', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useConnectionStore.getState().disconnect(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('disconnect_smcp', { instanceId });
      expect(mockedInvoke).toHaveBeenCalledTimes(1);
      expect(mockedDebug).toHaveBeenCalledWith(expect.stringContaining(
        'connection.request_completed layer=frontend operation=disconnect instance_id=computer-a',
      ));
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('disconnect failed');

      await expect(useConnectionStore.getState().disconnect(instanceId)).rejects.toBe(
        'disconnect failed',
      );

      expect(useConnectionStore.getState().error).toBe('disconnect failed');
      expect(useConnectionStore.getState().loading).toBe(false);
      expect(mockedLogError).toHaveBeenCalledWith(expect.stringContaining(
        'connection.request_failed layer=frontend operation=disconnect instance_id=computer-a',
      ));
    });
  });
});
