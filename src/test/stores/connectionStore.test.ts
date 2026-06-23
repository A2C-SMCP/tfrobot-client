import { invoke } from '@tauri-apps/api/core';
import { useConnectionStore, type ConnectionProfile } from '@/stores/connectionStore';

const mockedInvoke = vi.mocked(invoke);
const instanceId = 'computer-a';

function resetStore() {
  useConnectionStore.setState({
    profiles: [],
    statuses: {},
    loading: false,
    error: null,
  });
}

const mockProfile: ConnectionProfile = {
  name: 'dev',
  url: 'https://smcp.example.com',
  namespace: '/',
  office_id: 'office-1',
  computer_name: 'my-pc',
  headers: {},
};

describe('connectionStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchProfiles', () => {
    it('populates profiles list', async () => {
      mockedInvoke.mockResolvedValueOnce([mockProfile]);

      await useConnectionStore.getState().fetchProfiles(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('list_profiles', { instanceId });
      expect(useConnectionStore.getState().profiles).toEqual([mockProfile]);
      expect(useConnectionStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('network error');

      await useConnectionStore.getState().fetchProfiles(instanceId);

      expect(useConnectionStore.getState().error).toBe('network error');
    });
  });

  describe('fetchStatus', () => {
    it('updates connection status', async () => {
      const status = { connected: true, url: 'https://smcp.example.com', profile_name: 'dev' };
      mockedInvoke.mockResolvedValueOnce(status);

      await useConnectionStore.getState().fetchStatus(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('get_connection_status', { instanceId });
      expect(useConnectionStore.getState().getStatus(instanceId)).toEqual(status);
    });

    it('keeps connection status isolated by instance', async () => {
      mockedInvoke
        .mockResolvedValueOnce({ connected: true, profile_name: 'prod-a' })
        .mockResolvedValueOnce({ connected: false });

      await useConnectionStore.getState().fetchStatus('computer-a');
      await useConnectionStore.getState().fetchStatus('computer-b');

      expect(useConnectionStore.getState().getStatus('computer-a')).toEqual({
        connected: true,
        profile_name: 'prod-a',
      });
      expect(useConnectionStore.getState().getStatus('computer-b')).toEqual({
        connected: false,
      });
    });
  });

  describe('saveProfile', () => {
    it('invokes save_profile with apiKey and refreshes', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);       // save_profile
      mockedInvoke.mockResolvedValueOnce([mockProfile]);   // fetchProfiles

      await useConnectionStore.getState().saveProfile(instanceId, mockProfile, 'secret-key');

      expect(mockedInvoke).toHaveBeenCalledWith('save_profile', {
        instanceId,
        profile: mockProfile,
        apiKey: 'secret-key',
      });
    });

    it('passes null apiKey when omitted', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useConnectionStore.getState().saveProfile(instanceId, mockProfile);

      expect(mockedInvoke).toHaveBeenCalledWith('save_profile', {
        instanceId,
        profile: mockProfile,
        apiKey: null,
      });
    });
  });

  describe('deleteProfile', () => {
    it('invokes delete_profile and refreshes', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useConnectionStore.getState().deleteProfile(instanceId, 'dev');

      expect(mockedInvoke).toHaveBeenCalledWith('delete_profile', { instanceId, name: 'dev' });
    });
  });

  describe('connect / disconnect', () => {
    it('connect invokes connect_smcp and refreshes status', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);                          // connect_smcp
      mockedInvoke.mockResolvedValueOnce({ connected: true, url: 'x' });     // fetchStatus
      mockedInvoke.mockResolvedValueOnce([]);                                // fetchInstances

      await useConnectionStore.getState().connect(instanceId, 'dev');

      expect(mockedInvoke).toHaveBeenCalledWith('connect_smcp', { instanceId, profileName: 'dev' });
      expect(useConnectionStore.getState().loading).toBe(false);
    });

    it('disconnect invokes disconnect_smcp and refreshes status', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce({ connected: false });
      mockedInvoke.mockResolvedValueOnce([]);

      await useConnectionStore.getState().disconnect(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('disconnect_smcp', { instanceId });
    });

    it('connect sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('auth failed');

      await expect(
        useConnectionStore.getState().connect(instanceId, 'dev')
      ).rejects.toBe('auth failed');

      expect(useConnectionStore.getState().error).toBe('auth failed');
      expect(useConnectionStore.getState().loading).toBe(false);
    });
  });
});
