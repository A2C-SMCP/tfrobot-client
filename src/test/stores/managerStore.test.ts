import { invoke } from '@tauri-apps/api/core';
import {
  currentEmployeeResource,
  managerContextScope,
  managerSessionFromContext,
  useManagerStore,
  type AccountOption,
  type DigitalEmployeeBrief,
  type LoginResult,
  type ManagerContextSnapshot,
  type ManagerAccountSummary,
  type ManagerError,
} from '@/stores/managerStore';
import { useComputerStore } from '@/stores/computerStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

const employeeA: DigitalEmployeeBrief = {
  id: 11,
  name: 'bot-one',
  robotId: 'robot-a',
  robotAccountId: '4242',
  namespace: 'ns-a',
  status: 'running',
  departments: [{
    id: '7',
    name: '平台组',
    ancestors: [
      { id: '1', name: '总公司' },
      { id: '7', name: '平台组' },
    ],
  }],
};

const employeeB: DigitalEmployeeBrief = {
  id: 11,
  name: 'same-id-other-account',
  robotId: 'robot-b',
  robotAccountId: '5252',
  status: 'running',
};

function authenticatedContext(
  revision: number,
  accountId = 'account-a',
  organizationId = 'organization-a',
): ManagerContextSnapshot {
  return {
    revision,
    authState: 'authenticated',
    environment: 'staging',
    contextKey: { environment: 'staging', accountId, organizationId },
    user: { id: 'user-9', nickname: 'User', email: '', phone: '' },
    account: { id: accountId, name: `name-${accountId}`, nickname: 'User', avatar: '', employeeNo: '' },
    organization: { id: organizationId, name: `name-${organizationId}`, organizationType: 'team' },
    permissions: ['robot:read'],
  };
}

function signedOutContext(revision: number): ManagerContextSnapshot {
  return {
    revision,
    authState: 'signed_out',
    environment: null,
    contextKey: null,
    user: null,
    account: null,
    organization: null,
    permissions: [],
  };
}

function accountSelectionContext(revision: number): ManagerContextSnapshot {
  return {
    ...signedOutContext(revision),
    authState: 'account_selection_required',
    environment: 'beta',
  };
}

function setContextWithEmployees(
  context: ManagerContextSnapshot,
  employees: DigitalEmployeeBrief[],
  overrides: Partial<NonNullable<ReturnType<typeof currentEmployeeResource>>> = {},
) {
  useManagerStore.getState().applyContext(context);
  const scope = managerContextScope(context);
  if (scope === null) throw new Error('authenticated context required');
  useManagerStore.setState((state) => ({
    employeeResources: {
      ...state.employeeResources,
      [scope]: {
        ...state.employeeResources[scope],
        employees,
        ...overrides,
      },
    },
  }));
  return scope;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe('managerStore authoritative Context', () => {
  beforeEach(() => {
    useManagerStore.setState(useManagerStore.getInitialState(), true);
    useComputerStore.setState(useComputerStore.getInitialState(), true);
    mockedInvoke.mockReset();
  });

  it('accepts only monotonic backend snapshots and derives the legacy display session', () => {
    const revision2 = authenticatedContext(2);
    expect(useManagerStore.getState().applyContext(revision2)).toBe(true);
    expect(managerSessionFromContext(useManagerStore.getState().context)).toEqual({
      userId: 'user-9',
      accountId: 'account-a',
      accountName: 'name-account-a',
    });

    expect(useManagerStore.getState().applyContext(signedOutContext(1))).toBe(false);
    expect(useManagerStore.getState().context).toEqual(revision2);

    const conflictingRevision2 = authenticatedContext(2, 'account-b', 'organization-b');
    expect(useManagerStore.getState().applyContext(conflictingRevision2)).toBe(false);
    expect(useManagerStore.getState().context).toEqual(revision2);
  });

  it('rejects an authenticated snapshot whose ContextKey disagrees with identity fields', () => {
    const invalid = authenticatedContext(1);
    invalid.contextKey = { ...invalid.contextKey!, accountId: 'wrong-account' };

    expect(useManagerStore.getState().applyContext(invalid)).toBe(false);
    expect(useManagerStore.getState().context.authState).toBe('signed_out');
    expect(useManagerStore.getState().identityError).toMatchObject({ kind: 'invalid_response' });
  });

  it('uses manager_get_context as login authority instead of the login result projection', async () => {
    const loginResult: LoginResult = {
      kind: 'authenticated',
      user: { userId: 'untrusted', accountId: 'untrusted', accountName: 'untrusted' },
    };
    const context = authenticatedContext(1);
    mockedInvoke
      .mockResolvedValueOnce(loginResult)
      .mockResolvedValueOnce(context);

    await useManagerStore.getState().login('staging', 'user@example.com', 'secret');

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'manager_login', {
      environment: 'staging',
      identifier: 'user@example.com',
      password: 'secret',
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'manager_get_context');
    expect(useManagerStore.getState().context).toEqual(context);
    expect(managerSessionFromContext(useManagerStore.getState().context)?.accountId).toBe('account-a');
  });

  it('keeps account candidates as transient data only while backend Context requires selection', async () => {
    const accounts: AccountOption[] = [{
      accountId: 'account-b',
      accountName: 'Account B',
      nickname: 'B',
      organizationId: 'organization-b',
      organizationName: 'Organization B',
      organizationType: 'team',
    }];
    mockedInvoke
      .mockResolvedValueOnce({ kind: 'account_selection_required', accounts } satisfies LoginResult)
      .mockResolvedValueOnce(accountSelectionContext(1));

    await useManagerStore.getState().login('beta', 'user@example.com', 'secret');
    expect(useManagerStore.getState().pendingAccountSelection).toEqual(accounts);

    useManagerStore.getState().applyContext(authenticatedContext(2, 'account-b', 'organization-b'));
    expect(useManagerStore.getState().pendingAccountSelection).toBeNull();
  });

  it('restores only after reconciling the backend Context snapshot', async () => {
    const context = authenticatedContext(3);
    mockedInvoke
      .mockResolvedValueOnce({
        environment: 'staging',
        user: { userId: 'legacy', accountId: 'legacy', accountName: 'legacy' },
      })
      .mockResolvedValueOnce(context);

    await useManagerStore.getState().restoreSession();

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'manager_restore_session');
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'manager_get_context');
    expect(useManagerStore.getState().context).toEqual(context);
    expect(useManagerStore.getState().restoreAttempted).toBe(true);
  });

  it('scopes the switch-account directory to the authoritative Context revision', async () => {
    const contextA = authenticatedContext(1);
    useManagerStore.getState().applyContext(contextA);
    const accounts: ManagerAccountSummary[] = [{
      accountId: 'account-b',
      accountName: 'Account B',
      nickname: 'B',
      organizationId: 'organization-b',
      organizationName: 'Organization B',
      organizationType: 'team',
      role: 'member',
    }];
    mockedInvoke.mockResolvedValueOnce(accounts);

    await expect(useManagerStore.getState().fetchAccounts()).resolves.toEqual(accounts);
    expect(mockedInvoke).toHaveBeenCalledWith('manager_list_accounts');
    expect(useManagerStore.getState().availableAccounts).toEqual(accounts);
    expect(useManagerStore.getState().accountDirectoryScope).toBe(managerContextScope(contextA));

    useManagerStore.getState().applyContext(
      authenticatedContext(2, 'account-b', 'organization-b'),
    );
    expect(useManagerStore.getState().availableAccounts).toBeNull();
    expect(useManagerStore.getState().accountDirectoryScope).toBeNull();
  });

  it('drops an account directory response from the departing Context', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    const request = deferred<ManagerAccountSummary[]>();
    mockedInvoke.mockReturnValueOnce(request.promise);
    const fetchPromise = useManagerStore.getState().fetchAccounts();

    useManagerStore.getState().applyContext(
      authenticatedContext(2, 'account-b', 'organization-b'),
    );
    request.resolve([{
      accountId: 'old-account',
      accountName: 'Old Account',
      nickname: 'Old',
      organizationId: 'old-organization',
      organizationName: 'Old Organization',
      organizationType: 'team',
      role: 'owner',
    }]);

    await expect(fetchPromise).resolves.toEqual([]);
    expect(useManagerStore.getState().availableAccounts).toBeNull();
  });

  it('switches accounts through the backend transaction and accepts only its Context snapshot', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    const switched = authenticatedContext(2, 'account-b', 'organization-b');
    mockedInvoke
      .mockResolvedValueOnce({ ignoredIdentity: 'not-authoritative' })
      .mockResolvedValueOnce(switched);

    await useManagerStore.getState().switchAccount('account-b');

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'manager_switch_account', {
      accountId: 'account-b',
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'manager_get_context');
    expect(useManagerStore.getState().context).toEqual(switched);
    expect(useManagerStore.getState().identityLoading).toBe(false);
  });

  it('reconciles signed-out Context even when switch reports cleanup or auth failure', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    const unauthorized = { kind: 'unauthorized' } satisfies ManagerError;
    mockedInvoke
      .mockRejectedValueOnce(unauthorized)
      .mockResolvedValueOnce(signedOutContext(2));

    await expect(useManagerStore.getState().switchAccount('account-b')).rejects.toEqual(
      unauthorized,
    );

    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'manager_get_context');
    expect(useManagerStore.getState().context).toEqual(signedOutContext(2));
    expect(useManagerStore.getState().identityError).toEqual(unauthorized);
  });

  it('scopes employee lists by ContextKey plus revision, including identical employee IDs', async () => {
    const contextA = authenticatedContext(1);
    useManagerStore.getState().applyContext(contextA);
    mockedInvoke.mockResolvedValueOnce([employeeA]);
    await useManagerStore.getState().fetchEmployees();
    const scopeA = managerContextScope(contextA)!;

    const contextB = authenticatedContext(2, 'account-b', 'organization-b');
    useManagerStore.getState().applyContext(contextB);
    mockedInvoke.mockResolvedValueOnce([employeeB]);
    await useManagerStore.getState().fetchEmployees();
    const scopeB = managerContextScope(contextB)!;

    const state = useManagerStore.getState();
    expect(scopeA).not.toBe(scopeB);
    expect(state.employeeResources[scopeA].employees).toEqual([employeeA]);
    expect(state.employeeResources[scopeB].employees).toEqual([employeeB]);
    expect(currentEmployeeResource(state)?.employees).toEqual([employeeB]);
  });

  it('creates a fresh resource scope for a same-account revision', () => {
    const first = authenticatedContext(1);
    const firstScope = setContextWithEmployees(first, [employeeA]);
    const relogin = authenticatedContext(2);

    useManagerStore.getState().applyContext(relogin);

    const secondScope = managerContextScope(relogin)!;
    expect(secondScope).not.toBe(firstScope);
    expect(useManagerStore.getState().employeeResources[firstScope].employees).toEqual([employeeA]);
    expect(useManagerStore.getState().employeeResources[secondScope].employees).toEqual([]);
  });

  it('drops a successful employee response that resolves after an account switch', async () => {
    const contextA = authenticatedContext(1);
    useManagerStore.getState().applyContext(contextA);
    const request = deferred<DigitalEmployeeBrief[]>();
    mockedInvoke.mockReturnValueOnce(request.promise);
    const fetchPromise = useManagerStore.getState().fetchEmployees();

    const contextB = authenticatedContext(2, 'account-b', 'organization-b');
    useManagerStore.getState().applyContext(contextB);
    request.resolve([employeeA]);
    await fetchPromise;

    expect(currentEmployeeResource(useManagerStore.getState())?.employees).toEqual([]);
  });

  it('drops a failed employee response that resolves after an account switch', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    const request = deferred<DigitalEmployeeBrief[]>();
    mockedInvoke.mockReturnValueOnce(request.promise);
    const fetchPromise = useManagerStore.getState().fetchEmployees();

    useManagerStore.getState().applyContext(authenticatedContext(2, 'account-b', 'organization-b'));
    request.reject({ kind: 'forbidden' } satisfies ManagerError);
    await expect(fetchPromise).resolves.toBeUndefined();

    expect(currentEmployeeResource(useManagerStore.getState())?.error).toBeNull();
    expect(useManagerStore.getState().identityError).toBeNull();
  });

  it('keeps offline data and errors inside the current resource scope', async () => {
    const context = authenticatedContext(1);
    setContextWithEmployees(context, [employeeA]);
    const error: ManagerError = { kind: 'network_error', detail: 'offline' };
    mockedInvoke.mockRejectedValueOnce(error);

    await expect(useManagerStore.getState().fetchEmployees()).rejects.toEqual(error);

    const resource = currentEmployeeResource(useManagerStore.getState());
    expect(resource?.employees).toEqual([employeeA]);
    expect(resource?.error).toEqual(error);
    expect(useManagerStore.getState().online).toBe(false);
  });

  it('treats a successful empty employee list as fresh', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    mockedInvoke.mockResolvedValueOnce([]);

    await useManagerStore.getState().fetchEmployeesIfStale();
    await useManagerStore.getState().fetchEmployeesIfStale();

    expect(mockedInvoke).toHaveBeenCalledTimes(1);
    expect(mockedInvoke).toHaveBeenCalledWith('manager_list_digital_employees');
    expect(currentEmployeeResource(useManagerStore.getState())).toMatchObject({
      employees: [],
      loading: false,
    });
  });

  it('tracks payment and selection state in the revisioned resource', async () => {
    setContextWithEmployees(authenticatedContext(1), [employeeA]);
    const error: ManagerError = {
      kind: 'payment_required',
      detail: { message: 'Quota exceeded', redirect_url: 'https://pay.example.com' },
    };
    mockedInvoke.mockRejectedValueOnce(error);

    await expect(
      useManagerStore.getState().selectEmployeeAndConnect('computer-a', employeeA.id),
    ).rejects.toEqual(error);

    expect(currentEmployeeResource(useManagerStore.getState())).toMatchObject({
      selectedEmployeeId: employeeA.id,
      connectingEmployeeId: null,
      paymentRequired: { message: 'Quota exceeded', redirectUrl: 'https://pay.example.com' },
      error,
    });
  });

  it('connects using the employee in the captured resource and reconciles metadata', async () => {
    setContextWithEmployees(authenticatedContext(1), [employeeA]);
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
    mockedInvoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([{
        id: 'computer-a',
        name: 'Computer A',
        running: true,
        runtime: runtimeSnapshot({ lifecycle: 'started' }),
        connected: false,
        mcp_server_count: 0,
      }]);

    await expect(
      useManagerStore.getState().selectEmployeeAndConnect('computer-a', employeeA.id),
    ).resolves.toEqual({ name: employeeA.name });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'manager_connect_smcp', {
      instanceId: 'computer-a',
      employeeId: employeeA.id,
      scope: null,
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'list_computer_instances');
  });

  it('auth-expired never synthesizes identity and reconciles the backend snapshot', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    mockedInvoke.mockResolvedValueOnce(signedOutContext(2));

    await useManagerStore.getState().handleAuthExpired();

    expect(mockedInvoke).toHaveBeenCalledWith('manager_get_context');
    expect(useManagerStore.getState().context).toEqual(signedOutContext(2));
    expect(useManagerStore.getState().identityError).toEqual({ kind: 'unauthorized' });
  });

  it('logout commits the backend signed-out Context without mutating it locally', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    mockedInvoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(signedOutContext(2));

    await useManagerStore.getState().logout();

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'manager_logout');
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'manager_get_context');
    expect(useManagerStore.getState().context).toEqual(signedOutContext(2));
  });

  it('reconciles signed-out Context when logout completes with cleanup diagnostics', async () => {
    useManagerStore.getState().applyContext(authenticatedContext(1));
    const diagnostic: ManagerError = {
      kind: 'other',
      detail: { status: 0, body: 'Manager logout completed locally with cleanup diagnostics' },
    };
    mockedInvoke
      .mockRejectedValueOnce(diagnostic)
      .mockResolvedValueOnce(signedOutContext(2));

    await expect(useManagerStore.getState().logout()).rejects.toEqual(diagnostic);

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'manager_logout');
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'manager_get_context');
    expect(useManagerStore.getState().context).toEqual(signedOutContext(2));
    expect(useManagerStore.getState().identityError).toEqual(diagnostic);
  });
});
