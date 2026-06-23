import { invoke } from '@tauri-apps/api/core';
import { useConnectionStore } from '@/stores/connectionStore';

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
    mockedInvoke.mockReset();
  });

  describe('fetchStatus', () => {
    it('updates connection status', async () => {
      const status = { connected: true, url: 'https://smcp.example.com', target_name: 'dev' };
      mockedInvoke.mockResolvedValueOnce(status);

      await useConnectionStore.getState().fetchStatus(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('get_connection_status', { instanceId });
      expect(useConnectionStore.getState().getStatus(instanceId)).toEqual(status);
    });

    it('keeps connection status isolated by instance', async () => {
      mockedInvoke
        .mockResolvedValueOnce({ connected: true, target_name: 'prod-a' })
        .mockResolvedValueOnce({ connected: false });

      await useConnectionStore.getState().fetchStatus('computer-a');
      await useConnectionStore.getState().fetchStatus('computer-b');

      expect(useConnectionStore.getState().getStatus('computer-a')).toEqual({
        connected: true,
        target_name: 'prod-a',
      });
      expect(useConnectionStore.getState().getStatus('computer-b')).toEqual({
        connected: false,
      });
    });
  });

  describe('disconnect', () => {
    it('invokes disconnect_smcp and refreshes status', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce({ connected: false });
      mockedInvoke.mockResolvedValueOnce([]);

      await useConnectionStore.getState().disconnect(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('disconnect_smcp', { instanceId });
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
