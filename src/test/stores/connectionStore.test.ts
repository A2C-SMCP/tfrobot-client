import { invoke } from '@tauri-apps/api/core';
import { useConnectionStore, type ConnectionProfile } from '@/stores/connectionStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useConnectionStore.setState({
    profiles: [],
    status: { connected: false },
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
  auto_connect: false,
  auto_reconnect: true,
};

describe('connectionStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchProfiles', () => {
    it('populates profiles list', async () => {
      mockedInvoke.mockResolvedValueOnce([mockProfile]);

      await useConnectionStore.getState().fetchProfiles();

      expect(mockedInvoke).toHaveBeenCalledWith('list_profiles');
      expect(useConnectionStore.getState().profiles).toEqual([mockProfile]);
      expect(useConnectionStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('network error');

      await useConnectionStore.getState().fetchProfiles();

      expect(useConnectionStore.getState().error).toBe('network error');
    });
  });

  describe('fetchStatus', () => {
    it('updates connection status', async () => {
      const status = { connected: true, url: 'https://smcp.example.com', profile_name: 'dev' };
      mockedInvoke.mockResolvedValueOnce(status);

      await useConnectionStore.getState().fetchStatus();

      expect(mockedInvoke).toHaveBeenCalledWith('get_connection_status');
      expect(useConnectionStore.getState().status).toEqual(status);
    });
  });

  describe('saveProfile', () => {
    it('invokes save_profile with apiKey and refreshes', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);       // save_profile
      mockedInvoke.mockResolvedValueOnce([mockProfile]);   // fetchProfiles

      await useConnectionStore.getState().saveProfile(mockProfile, 'secret-key');

      expect(mockedInvoke).toHaveBeenCalledWith('save_profile', {
        profile: mockProfile,
        apiKey: 'secret-key',
      });
    });

    it('passes null apiKey when omitted', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useConnectionStore.getState().saveProfile(mockProfile);

      expect(mockedInvoke).toHaveBeenCalledWith('save_profile', {
        profile: mockProfile,
        apiKey: null,
      });
    });
  });

  describe('deleteProfile', () => {
    it('invokes delete_profile and refreshes', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useConnectionStore.getState().deleteProfile('dev');

      expect(mockedInvoke).toHaveBeenCalledWith('delete_profile', { name: 'dev' });
    });
  });

  describe('connect / disconnect', () => {
    it('connect invokes connect_smcp and refreshes status', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);                          // connect_smcp
      mockedInvoke.mockResolvedValueOnce({ connected: true, url: 'x' });     // fetchStatus

      await useConnectionStore.getState().connect('dev');

      expect(mockedInvoke).toHaveBeenCalledWith('connect_smcp', { profileName: 'dev' });
      expect(useConnectionStore.getState().loading).toBe(false);
    });

    it('disconnect invokes disconnect_smcp and refreshes status', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce({ connected: false });

      await useConnectionStore.getState().disconnect();

      expect(mockedInvoke).toHaveBeenCalledWith('disconnect_smcp');
    });

    it('connect sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('auth failed');

      await expect(
        useConnectionStore.getState().connect('dev')
      ).rejects.toBe('auth failed');

      expect(useConnectionStore.getState().error).toBe('auth failed');
      expect(useConnectionStore.getState().loading).toBe(false);
    });
  });
});
