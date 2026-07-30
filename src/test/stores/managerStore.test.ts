import { invoke } from '@tauri-apps/api/core';
import {
  useManagerStore,
  type DigitalEmployeeBrief,
  type LoginResult,
  type ManagerError,
  type UserInfo,
} from '@/stores/managerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import { useComputerStore } from '@/stores/computerStore';
import { resetAllStores, runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

const user: UserInfo = { userId: 9, accountId: 16, accountName: 'client_uat' };

const employeeA: DigitalEmployeeBrief = {
  id: 11,
  name: 'bot-one',
  robotId: 'robot-a',
  robotAccountId: 4242,
  namespace: 'ns-a',
  templateType: 'tfrserver',
  status: 'running',
  departments: [
    {
      id: 7,
      name: '平台组',
      path: '/1/3/7/',
      ancestors: [
        { id: 1, name: '总公司' },
        { id: 3, name: '研发中心' },
        { id: 7, name: '平台组' },
      ],
    },
  ],
};

const employeeB: DigitalEmployeeBrief = {
  id: 12,
  name: 'bot-two',
  robotId: 'robot-b',
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

  describe('restoreSession', () => {
    it('restores session from persisted Manager credentials', async () => {
      mockedInvoke.mockResolvedValueOnce({
        baseUrl: 'https://manager.example.com',
        user,
      });

      const ret = await useManagerStore.getState().restoreSession();

      expect(mockedInvoke).toHaveBeenCalledWith('manager_restore_session');
      expect(ret).toEqual({ baseUrl: 'https://manager.example.com', user });
      expect(useManagerStore.getState().session).toEqual(user);
      expect(useManagerStore.getState().baseUrl).toBe('https://manager.example.com');
      expect(useManagerStore.getState().restoreAttempted).toBe(true);
    });

    it('marks restore attempted when no persisted session exists', async () => {
      mockedInvoke.mockResolvedValueOnce(null);

      const ret = await useManagerStore.getState().restoreSession();

      expect(ret).toBeNull();
      expect(useManagerStore.getState().session).toBeNull();
      expect(useManagerStore.getState().restoreAttempted).toBe(true);
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

    it('invokes manager_connect_smcp with robotAccountId and returns the robot name', async () => {
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
      // 后端编排全路径（exchange + connect + 预刷新）为单条命令。
      mockedInvoke.mockResolvedValueOnce(undefined); // manager_connect_smcp
      mockedInvoke.mockResolvedValueOnce([{
        id: 'computer-a',
        name: 'Computer A',
        running: true,
        runtime: runtimeSnapshot({ lifecycle: 'joined_office' }),
        connected: true,
        client_connection_present: true,
        connection_revision: 9,
        connection_context: {
          profile_name: 'manager:11',
          url: 'https://smcp.example.com',
          office_id: 'office-a',
          computer_name: 'Computer A',
          connected_at: '2026-07-17T00:00:00Z',
          source_type: 'manager_robot',
          employee_id: 11,
        },
        mcp_server_count: 0,
        robot_binding: {
          employee_id: 11,
          robot_id: 'robot-a',
          robot_account_id: 4242,
          namespace: 'ns-a',
          robot_name: 'bot-one',
        },
        connection_policy: {
          target: { type: 'manager_robot', id: '11', robotAccountId: 4242 },
          auto_connect: false,
        },
      }]); // metadata-only reconciliation

      const ret = await useManagerStore.getState().selectEmployeeAndConnect('computer-a', 11);

      expect(ret).toEqual({ name: 'bot-one' });
      expect(mockedInvoke).toHaveBeenCalledWith('manager_connect_smcp', {
        instanceId: 'computer-a',
        employeeId: 11,
        robotAccountId: 4242,
        robotId: 'robot-a',
        robotName: 'bot-one',
        namespace: 'ns-a',
        scope: null,
      });
      expect(mockedInvoke).toHaveBeenCalledWith('list_computer_instances');
      expect(mockedInvoke).not.toHaveBeenCalledWith('get_connection_status', expect.anything());
      expect(useConnectionStore.getState().getStatus('computer-a')).toEqual({ connected: false });
      expect(useComputerStore.getState().instances[0]).toMatchObject({
        connectionStatus: 'disconnected',
        robotName: 'bot-one',
        robotBinding: { employee_id: 11, robot_account_id: 4242 },
        connectionPolicy: {
          target: { type: 'manager_robot', id: '11', robotAccountId: 4242 },
          auto_connect: false,
        },
      });
      // 不再走 profile 构建/保存/connect_smcp 老路径。
      expect(mockedInvoke).not.toHaveBeenCalledWith('save_profile', expect.anything());
      expect(mockedInvoke).not.toHaveBeenCalledWith('connect_smcp', expect.anything());
    });

    it('surfaces a partial-success warning when Manager metadata refresh fails', async () => {
      mockedInvoke
        .mockResolvedValueOnce(undefined)
        .mockRejectedValueOnce('metadata unavailable');

      await expect(
        useManagerStore.getState().selectEmployeeAndConnect('computer-a', 11),
      ).rejects.toMatchObject({
        kind: 'other',
        detail: {
          status: 0,
          body: expect.stringContaining(
            'Connection succeeded, but refreshing its saved binding and policy failed',
          ),
        },
      });

      expect(useManagerStore.getState().error).toMatchObject({
        kind: 'other',
        detail: { status: 0 },
      });
    });

    it('rejects without calling the backend when robotAccountId is missing', async () => {
      // employeeB 没有 robotAccountId（历史实例）→ 前置校验直接拒，不发命令。
      useManagerStore.setState({ employees: [employeeB] });

      await expect(
        useManagerStore.getState().selectEmployeeAndConnect('computer-a', employeeB.id),
      ).rejects.toMatchObject({ kind: 'invalid_response' });
      expect(mockedInvoke).not.toHaveBeenCalled();
    });

    it('surfaces payment_required detail', async () => {
      const err: ManagerError = {
        kind: 'payment_required',
        detail: { message: 'Quota exceeded', redirect_url: 'https://pay.example.com' },
      };
      mockedInvoke.mockRejectedValueOnce(err); // manager_connect_smcp

      await expect(
        useManagerStore.getState().selectEmployeeAndConnect('computer-a', 11),
      ).rejects.toEqual(err);
      expect(useManagerStore.getState().paymentRequired).toEqual({
        message: 'Quota exceeded',
        redirectUrl: 'https://pay.example.com',
      });
    });

    it('surfaces signing_unavailable when token signing is not ready', async () => {
      const err: ManagerError = {
        kind: 'signing_unavailable',
        detail: { message: 'keys not provisioned' },
      };
      mockedInvoke.mockRejectedValueOnce(err); // manager_connect_smcp

      await expect(
        useManagerStore.getState().selectEmployeeAndConnect('computer-a', 11),
      ).rejects.toEqual(err);
      expect(useManagerStore.getState().error).toEqual(err);
    });
  });

  describe('department visibility & staleness (TFRM-56)', () => {
    it('fetchEmployees stores departments, lastFetchAt and marks online', async () => {
      useManagerStore.setState({ session: user });
      mockedInvoke.mockResolvedValueOnce([employeeA]);

      await useManagerStore.getState().fetchEmployees();

      const s = useManagerStore.getState();
      expect(s.employees[0].departments?.[0].ancestors?.[2].name).toBe('平台组');
      expect(typeof s.lastFetchAt).toBe('number');
      expect(s.online).toBe(true);
    });

    it('fetchEmployees keeps the last list and marks offline on network_error', async () => {
      useManagerStore.setState({ session: user, employees: [employeeA], online: true });
      const err: ManagerError = { kind: 'network_error', detail: 'connect refused' };
      mockedInvoke.mockRejectedValueOnce(err);

      await expect(useManagerStore.getState().fetchEmployees()).rejects.toEqual(err);

      const s = useManagerStore.getState();
      expect(s.employees).toEqual([employeeA]); // 离线保留最近一次成功列表
      expect(s.online).toBe(false);
    });

    it('fetchEmployeesIfStale skips refetch when within the freshness window', async () => {
      useManagerStore.setState({
        session: user,
        employees: [employeeA],
        lastFetchAt: Date.now(),
      });

      await useManagerStore.getState().fetchEmployeesIfStale();

      expect(mockedInvoke).not.toHaveBeenCalled();
    });

    it('fetchEmployeesIfStale refetches when older than 60s', async () => {
      useManagerStore.setState({
        session: user,
        employees: [employeeA],
        lastFetchAt: Date.now() - 61_000,
      });
      mockedInvoke.mockResolvedValueOnce([employeeA, employeeB]);

      await useManagerStore.getState().fetchEmployeesIfStale();

      expect(mockedInvoke).toHaveBeenCalledWith('manager_list_digital_employees');
      expect(useManagerStore.getState().employees).toHaveLength(2);
    });

    it('fetchEmployeesIfStale refetches when there is no data yet', async () => {
      useManagerStore.setState({ session: user, employees: [], lastFetchAt: null });
      mockedInvoke.mockResolvedValueOnce([employeeA]);

      await useManagerStore.getState().fetchEmployeesIfStale();

      expect(mockedInvoke).toHaveBeenCalledWith('manager_list_digital_employees');
    });

    it('setOnline recalibrates the list on offline -> online transition', async () => {
      useManagerStore.setState({ session: user, online: false, employees: [employeeA] });
      mockedInvoke.mockResolvedValueOnce([employeeA, employeeB]);

      useManagerStore.getState().setOnline(true);

      await vi.waitFor(() =>
        expect(mockedInvoke).toHaveBeenCalledWith('manager_list_digital_employees'),
      );
      expect(useManagerStore.getState().online).toBe(true);
    });

    it('setOnline does not refetch without a session', () => {
      useManagerStore.setState({ session: null, online: false });

      useManagerStore.getState().setOnline(true);

      expect(mockedInvoke).not.toHaveBeenCalled();
    });

    it('selectEmployeeAndConnect drops the item and recalibrates on visibility-revoked 404', async () => {
      useManagerStore.setState({ session: user, employees: [employeeA, employeeB] });
      const err: ManagerError = { kind: 'not_found_or_no_permission' };
      // manager_connect_smcp rejects with the revoked error (来自其内部 get_connection_info)
      mockedInvoke.mockRejectedValueOnce(err);
      // the calibration refetch returns the now-filtered server list
      mockedInvoke.mockResolvedValueOnce([employeeB]);

      await expect(
        useManagerStore.getState().selectEmployeeAndConnect('computer-a', employeeA.id),
      ).rejects.toEqual(err);

      // 列表已剔除不可见项 + refetch 校准
      await vi.waitFor(() =>
        expect(useManagerStore.getState().employees).toEqual([employeeB]),
      );
      expect(mockedInvoke).toHaveBeenCalledWith('manager_list_digital_employees');
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
    it('clears Manager session without disconnecting any Computer', async () => {
      useManagerStore.setState({ session: user, employees: [employeeA] });
      useConnectionStore.setState({
        ...useConnectionStore.getState(),
        statuses: {
          'computer-a': { connected: true, profile_name: 'bot-one' },
        },
      });
      // manager_logout
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useManagerStore.getState().logout();

      expect(mockedInvoke).not.toHaveBeenCalledWith('disconnect_smcp', expect.anything());
      expect(mockedInvoke).toHaveBeenCalledWith('manager_logout');
      expect(useManagerStore.getState().session).toBeNull();
      expect(useManagerStore.getState().employees).toEqual([]);
    });
  });
});
