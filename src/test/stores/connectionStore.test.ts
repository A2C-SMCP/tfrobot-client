import { invoke } from '@tauri-apps/api/core';
import { useConnectionStore } from '@/stores/connectionStore';
import {
  resetClientConnectionAuthorities,
  setClientConnectionAuthority,
} from '@/stores/connectionAuthority';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);
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
  });

  it('projects versioned connection authority with SDK lifecycle state', () => {
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
    expect(useConnectionStore.getState().getStatus(instanceId)).toEqual({
      connected: true,
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
      profile_name: 'prod',
      source_type: 'manual_smcp',
      target_id: 'target-a',
      target_name: undefined,
      employee_id: undefined,
    });

    useConnectionStore.getState().applyRuntimeSnapshot(
      instanceId,
      runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
    );
    expect(useConnectionStore.getState().getStatus(instanceId)).toEqual({ connected: false });
  });

  describe('disconnect', () => {
    it('invokes disconnect_smcp and lets runtime events update status', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useConnectionStore.getState().disconnect(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('disconnect_smcp', { instanceId });
      expect(mockedInvoke).toHaveBeenCalledTimes(1);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('disconnect failed');

      await expect(useConnectionStore.getState().disconnect(instanceId)).rejects.toBe(
        'disconnect failed',
      );

      expect(useConnectionStore.getState().error).toBe('disconnect failed');
      expect(useConnectionStore.getState().loading).toBe(false);
    });
  });
});
