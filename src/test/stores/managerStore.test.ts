import { invoke } from '@tauri-apps/api/core';
import {
  useManagerStore,
  type ConnectionInfo,
  type DigitalEmployeeBrief,
  type LoginResult,
  type ManagerError,
  type UserInfo,
} from '@/stores/managerStore';
import { useConnectionStore, type ConnectionProfile } from '@/stores/connectionStore';
import { resetAllStores } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

const user: UserInfo = { userId: 9, accountId: 16, accountName: 'client_uat' };

const employeeA: DigitalEmployeeBrief = {
  id: 11,
  name: 'bot-one',
  robotId: 'robot-a',
  namespace: 'ns-a',
  templateType: 'tfrserver',
  status: 'running',
};

const connInfo: ConnectionInfo = {
  socketBaseURL: 'wss://tfrserver.example.com',
  sioPath: '/socket.io/',
  namespace: 'ns-a',
  rid: 'robot-a',
  robotType: 'tfrobot',
  smcpNamespace: '/smcp',
  accessToken: 'super-secret-token',
  computerName: 'alice-laptop',
  routingHeaders: {
    'X-TF-Namespace': 'ns-a',
    'X-TF-RobotId': 'robot-a',
    'X-TF-RobotType': 'tfrobot',
    access_token: 'super-secret-token',
  },
  expiresAt: '2099-01-01T00:00:00Z',
};

describe('managerStore', () => {
  beforeEach(() => {
    resetAllStores();
    mockedInvoke.mockReset();
  });

  describe('login', () => {
    it('stores session on authenticated result', async () => {
      const result: LoginResult = { kind: 'authenticated', user };
      mockedInvoke.mockResolvedValueOnce(result);

      const ret = await useManagerStore
        .getState()
        .login('13800138008', 'Test@123456', 'http://localhost:8090');

      expect(mockedInvoke).toHaveBeenCalledWith('manager_login', {
        baseUrl: 'http://localhost:8090',
        phone: '13800138008',
        password: 'Test@123456',
      });
      expect(ret).toEqual(result);
      expect(useManagerStore.getState().session).toEqual(user);
      expect(useManagerStore.getState().baseUrl).toBe('http://localhost:8090');
      expect(useManagerStore.getState().pendingAccountSelection).toBeNull();
    });

    it('routes to account selection when multi-account', async () => {
      const result: LoginResult = {
        kind: 'account_selection_required',
        accounts: [
          {
            accountId: 2,
            accountName: 'testuser2_enterprise',
            nickname: '测试用户2',
            organizationId: 2,
            organizationName: '测试企业',
            organizationType: 'enterprise',
          },
        ],
      };
      mockedInvoke.mockResolvedValueOnce(result);

      await useManagerStore
        .getState()
        .login('13900139000', 'Test@123456', 'http://localhost:8090');

      expect(useManagerStore.getState().session).toBeNull();
      expect(useManagerStore.getState().pendingAccountSelection).toEqual(result.accounts);
    });

    it('stores unauthorized error and rethrows', async () => {
      const err: ManagerError = { kind: 'unauthorized' };
      mockedInvoke.mockRejectedValueOnce(err);

      await expect(
        useManagerStore.getState().login('13800138008', 'wrong', 'http://localhost:8090'),
      ).rejects.toEqual(err);
      expect(useManagerStore.getState().error).toEqual(err);
    });
  });

  describe('fetchEmployees', () => {
    it('populates the employees list', async () => {
      useManagerStore.setState({ session: user, baseUrl: 'https://mgr.example.com' });
      mockedInvoke.mockResolvedValueOnce([employeeA]);

      await useManagerStore.getState().fetchEmployees();

      expect(mockedInvoke).toHaveBeenCalledWith('manager_list_digital_employees');
      expect(useManagerStore.getState().employees).toEqual([employeeA]);
    });

    it('captures payment_required detail for UI modal', async () => {
      const err: ManagerError = {
        kind: 'payment_required',
        detail: { message: 'Balance low', redirect_url: 'https://pay.example.com' },
      };
      mockedInvoke.mockRejectedValueOnce(err);

      await expect(useManagerStore.getState().fetchEmployees()).rejects.toEqual(err);
      expect(useManagerStore.getState().paymentRequired).toEqual({
        message: 'Balance low',
        redirectUrl: 'https://pay.example.com',
      });
    });

    it('stores forbidden error', async () => {
      const err: ManagerError = { kind: 'forbidden' };
      mockedInvoke.mockRejectedValueOnce(err);

      await expect(useManagerStore.getState().fetchEmployees()).rejects.toEqual(err);
      expect(useManagerStore.getState().error).toEqual(err);
    });
  });

  describe('selectEmployeeAndConnect', () => {
    beforeEach(() => {
      useManagerStore.setState({
        session: user,
        employees: [employeeA],
      });
    });

    it('builds a profile from connection-info and triggers connect', async () => {
      // manager_get_connection_info
      mockedInvoke.mockResolvedValueOnce(connInfo);
      // list_profiles (inside fetchProfiles, no existing)
      mockedInvoke.mockResolvedValueOnce([]);
      // save_profile
      mockedInvoke.mockResolvedValueOnce(undefined);
      // list_profiles after save
      mockedInvoke.mockResolvedValueOnce([
        {
          name: 'bot-one',
          url: connInfo.socketBaseURL,
          namespace: '/smcp',
          office_id: 'robot-a',
          computer_name: 'alice-laptop',
          headers: connInfo.routingHeaders,
          auto_connect: false,
          auto_reconnect: false,
        } as ConnectionProfile,
      ]);
      // connect_smcp
      mockedInvoke.mockResolvedValueOnce(undefined);
      // get_connection_status after connect
      mockedInvoke.mockResolvedValueOnce({ connected: true, profile_name: 'bot-one' });

      const ret = await useManagerStore.getState().selectEmployeeAndConnect(11);

      expect(ret).toEqual({ profileName: 'bot-one' });
      expect(mockedInvoke).toHaveBeenCalledWith('manager_get_connection_info', { id: 11 });
      expect(mockedInvoke).toHaveBeenCalledWith('save_profile', expect.objectContaining({
        profile: expect.objectContaining({
          name: 'bot-one',
          url: connInfo.socketBaseURL,
          office_id: 'robot-a',
          headers: connInfo.routingHeaders,
          namespace: '/smcp',
        }),
        apiKey: null,
      }));
      expect(mockedInvoke).toHaveBeenCalledWith('connect_smcp', { profileName: 'bot-one' });
    });

    it('invokes onConflict when the profile name exists and honors copy decision', async () => {
      const existing: ConnectionProfile = {
        name: 'bot-one',
        url: 'old',
        namespace: '/smcp',
        office_id: 'old',
        computer_name: 'old',
        headers: {},
        auto_connect: false,
        auto_reconnect: false,
      };
      // manager_get_connection_info
      mockedInvoke.mockResolvedValueOnce(connInfo);
      // list_profiles (existing bot-one)
      mockedInvoke.mockResolvedValueOnce([existing]);
      // save_profile
      mockedInvoke.mockResolvedValueOnce(undefined);
      // list_profiles after save
      mockedInvoke.mockResolvedValueOnce([existing]);
      // connect_smcp
      mockedInvoke.mockResolvedValueOnce(undefined);
      // get_connection_status after connect
      mockedInvoke.mockResolvedValueOnce({ connected: true });

      const onConflict = vi.fn().mockResolvedValue('copy' as const);
      const ret = await useManagerStore
        .getState()
        .selectEmployeeAndConnect(11, onConflict);

      expect(onConflict).toHaveBeenCalledWith('bot-one');
      expect(ret?.profileName).toBe('bot-one (2)');
    });

    it('aborts when onConflict returns cancel', async () => {
      const existing: ConnectionProfile = {
        name: 'bot-one',
        url: '',
        namespace: '/smcp',
        office_id: '',
        computer_name: '',
        headers: {},
        auto_connect: false,
        auto_reconnect: false,
      };
      mockedInvoke.mockResolvedValueOnce(connInfo);
      mockedInvoke.mockResolvedValueOnce([existing]);

      const onConflict = vi.fn().mockResolvedValue('cancel' as const);
      const ret = await useManagerStore
        .getState()
        .selectEmployeeAndConnect(11, onConflict);

      expect(ret).toBeNull();
      // only get_connection_info + list_profiles — no save, no connect
      expect(mockedInvoke).toHaveBeenCalledTimes(2);
    });

    it('surfaces payment_required detail', async () => {
      const err: ManagerError = {
        kind: 'payment_required',
        detail: { message: 'Quota exceeded', redirect_url: 'https://pay.example.com' },
      };
      mockedInvoke.mockRejectedValueOnce(err);

      await expect(
        useManagerStore.getState().selectEmployeeAndConnect(11),
      ).rejects.toEqual(err);
      expect(useManagerStore.getState().paymentRequired).toEqual({
        message: 'Quota exceeded',
        redirectUrl: 'https://pay.example.com',
      });
    });
  });

  describe('handleAuthExpired', () => {
    it('clears session and stores unauthorized error', () => {
      useManagerStore.setState({ session: user, employees: [employeeA] });
      useManagerStore.getState().handleAuthExpired();
      expect(useManagerStore.getState().session).toBeNull();
      expect(useManagerStore.getState().employees).toEqual([]);
      expect(useManagerStore.getState().error).toEqual({ kind: 'unauthorized' });
    });
  });

  describe('logout', () => {
    it('disconnects active SMCP connection before clearing session', async () => {
      useManagerStore.setState({ session: user, employees: [employeeA] });
      useConnectionStore.setState({
        ...useConnectionStore.getState(),
        status: { connected: true, profile_name: 'bot-one' },
      });
      // disconnect_smcp
      mockedInvoke.mockResolvedValueOnce(undefined);
      // get_connection_status after disconnect
      mockedInvoke.mockResolvedValueOnce({ connected: false });
      // manager_logout
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useManagerStore.getState().logout();

      expect(mockedInvoke).toHaveBeenCalledWith('disconnect_smcp');
      expect(mockedInvoke).toHaveBeenCalledWith('manager_logout');
      expect(useManagerStore.getState().session).toBeNull();
      expect(useManagerStore.getState().employees).toEqual([]);
    });
  });
});
