import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info, warn, error as logError } from '@/utils/logger';
import { useComputerStore } from './computerStore';

/** Login command response. Identity authority always comes from ManagerContextSnapshot. */
export interface UserInfo {
  userId: string;
  accountId: string;
  accountName: string;
}

export interface AccountOption {
  accountId: string;
  accountName: string;
  nickname: string;
  organizationId: string;
  organizationName: string;
  organizationType: string;
}

export interface DepartmentAncestor {
  id: string;
  name: string;
}

export interface DepartmentRef {
  id: string;
  name: string;
  path?: string;
  ancestors?: DepartmentAncestor[];
}

export interface DigitalEmployeeBrief {
  id: number;
  name: string;
  description?: string;
  robotId?: string;
  robotAccountId?: string;
  status?: string;
  templateDisplayName?: string;
  templateType?: string;
  namespace?: string;
  clusterName?: string;
  departments?: DepartmentRef[];
}

export type LoginResult =
  | { kind: 'authenticated'; user: UserInfo }
  | { kind: 'account_selection_required'; accounts: AccountOption[] }
  | { kind: 'onboarding_required'; userId: string };

export type ManagerEnvironment = 'staging' | 'beta' | 'prod';
export type ManagerAuthState =
  | 'signed_out'
  | 'account_selection_required'
  | 'onboarding_required'
  | 'authenticated';

export interface ManagerContextKey {
  environment: ManagerEnvironment;
  accountId: string;
  organizationId: string;
}

export interface ManagerContextUser {
  id: string;
  nickname: string;
  email: string;
  phone: string;
}

export interface ManagerContextAccount {
  id: string;
  name: string;
  nickname: string;
  avatar: string;
  employeeNo: string;
}

export interface ManagerContextOrganization {
  id: string;
  name: string;
  organizationType: string;
}

/** Redacted, revisioned identity snapshot owned by the Rust coordinator. */
export interface ManagerContextSnapshot {
  revision: number;
  authState: ManagerAuthState;
  environment: ManagerEnvironment | null;
  contextKey: ManagerContextKey | null;
  user: ManagerContextUser | null;
  account: ManagerContextAccount | null;
  organization: ManagerContextOrganization | null;
  permissions: string[];
}

export interface RestoredManagerSession {
  environment: ManagerEnvironment;
  user: UserInfo;
}

export type ManagerError =
  | { kind: 'network_error'; detail: string }
  | { kind: 'unauthorized' }
  | { kind: 'invalid_credentials'; detail: { message: string } }
  | { kind: 'forbidden' }
  | { kind: 'payment_required'; detail: { message: string; redirect_url?: string } }
  | { kind: 'not_found' }
  | { kind: 'not_found_or_no_permission' }
  | { kind: 'other'; detail: { status: number; body: string } }
  | { kind: 'no_session' }
  | { kind: 'context_changed' }
  | { kind: 'missing_base_url' }
  | { kind: 'invalid_response'; detail: string }
  | { kind: 'keychain_error'; detail: string }
  | { kind: 'token_exchange'; detail: { error: string; description?: string } }
  | { kind: 'signing_unavailable'; detail: { message?: string } };

export interface PaymentRequiredInfo {
  message: string;
  redirectUrl?: string;
}

export interface ManagerEmployeeResource {
  scope: string;
  contextKey: ManagerContextKey;
  revision: number;
  employees: DigitalEmployeeBrief[];
  loading: boolean;
  error: ManagerError | null;
  paymentRequired: PaymentRequiredInfo | null;
  lastFetchAt: number | null;
  selectedEmployeeId: number | null;
  connectingEmployeeId: number | null;
}

export interface ManagerState {
  context: ManagerContextSnapshot;
  contextInitialized: boolean;
  pendingAccountSelection: AccountOption[] | null;
  identityLoading: boolean;
  restoreAttempted: boolean;
  identityError: ManagerError | null;
  employeeResources: Record<string, ManagerEmployeeResource>;
  online: boolean;

  applyContext: (snapshot: ManagerContextSnapshot) => boolean;
  refreshContext: () => Promise<ManagerContextSnapshot>;
  restoreSession: () => Promise<RestoredManagerSession | null>;
  login: (
    environment: ManagerEnvironment,
    identifier: string,
    password: string,
  ) => Promise<LoginResult>;
  selectAccount: (accountId: string) => Promise<void>;
  fetchEmployees: () => Promise<void>;
  fetchEmployeesIfStale: (maxAgeMs?: number) => Promise<void>;
  setOnline: (online: boolean) => void;
  selectEmployeeAndConnect: (
    instanceId: string,
    employeeId: number,
  ) => Promise<{ name: string } | null>;
  logout: () => Promise<void>;
  handleAuthExpired: () => Promise<void>;
  dismissPaymentRequired: () => void;
  clearError: () => void;
}

const EMPLOYEE_LIST_STALE_MS = 60_000;

export const SIGNED_OUT_MANAGER_CONTEXT: ManagerContextSnapshot = {
  revision: 0,
  authState: 'signed_out',
  environment: null,
  contextKey: null,
  user: null,
  account: null,
  organization: null,
  permissions: [],
};

const initialState = {
  context: SIGNED_OUT_MANAGER_CONTEXT,
  contextInitialized: false,
  pendingAccountSelection: null as AccountOption[] | null,
  identityLoading: false,
  restoreAttempted: false,
  identityError: null as ManagerError | null,
  employeeResources: {} as Record<string, ManagerEmployeeResource>,
  online: true,
};

export function managerContextScope(context: ManagerContextSnapshot): string | null {
  const key = context.contextKey;
  if (context.authState !== 'authenticated' || key === null) return null;
  return JSON.stringify([key.environment, key.accountId, key.organizationId, context.revision]);
}

export function managerSessionFromContext(context: ManagerContextSnapshot): UserInfo | null {
  if (
    context.authState !== 'authenticated'
    || context.user === null
    || context.account === null
  ) {
    return null;
  }
  return {
    userId: context.user.id,
    accountId: context.account.id,
    accountName: context.account.name,
  };
}

export function currentEmployeeResource(
  state: Pick<ManagerState, 'context' | 'employeeResources'>,
): ManagerEmployeeResource | null {
  const scope = managerContextScope(state.context);
  return scope === null ? null : state.employeeResources[scope] ?? null;
}

function createEmployeeResource(context: ManagerContextSnapshot): ManagerEmployeeResource | null {
  const scope = managerContextScope(context);
  if (scope === null || context.contextKey === null) return null;
  return {
    scope,
    contextKey: context.contextKey,
    revision: context.revision,
    employees: [],
    loading: false,
    error: null,
    paymentRequired: null,
    lastFetchAt: null,
    selectedEmployeeId: null,
    connectingEmployeeId: null,
  };
}

function contextIsValid(context: ManagerContextSnapshot): boolean {
  if (!Number.isSafeInteger(context.revision) || context.revision < 0) return false;
  if (context.authState !== 'authenticated') return context.contextKey === null;
  const { contextKey, environment, user, account, organization } = context;
  return Boolean(
    contextKey
      && environment
      && user?.id.trim()
      && account?.id.trim()
      && organization?.id.trim()
      && contextKey.environment === environment
      && contextKey.accountId === account.id
      && contextKey.organizationId === organization.id,
  );
}

function isManagerError(e: unknown): e is ManagerError {
  return typeof e === 'object' && e !== null && typeof (e as { kind?: unknown }).kind === 'string';
}

function toManagerError(e: unknown): ManagerError {
  if (isManagerError(e)) return e;
  return { kind: 'other', detail: { status: 0, body: String(e) } };
}

function updateResource(
  resources: Record<string, ManagerEmployeeResource>,
  scope: string,
  updater: (resource: ManagerEmployeeResource) => ManagerEmployeeResource,
): Record<string, ManagerEmployeeResource> {
  const resource = resources[scope];
  if (!resource) return resources;
  return { ...resources, [scope]: updater(resource) };
}

function requestStillCurrent(state: ManagerState, scope: string): boolean {
  return managerContextScope(state.context) === scope;
}

export const useManagerStore = create<ManagerState>((set, get) => ({
  ...initialState,

  applyContext: (snapshot) => {
    if (!contextIsValid(snapshot)) {
      set({
        identityError: {
          kind: 'invalid_response',
          detail: 'Manager Context snapshot violates its identity invariants',
        },
      });
      return false;
    }

    let accepted = false;
    set((state) => {
      if (snapshot.revision < state.context.revision) return state;
      if (state.contextInitialized && snapshot.revision === state.context.revision) return state;
      accepted = true;
      const resource = createEmployeeResource(snapshot);
      const employeeResources = resource && !state.employeeResources[resource.scope]
        ? { ...state.employeeResources, [resource.scope]: resource }
        : state.employeeResources;
      return {
        context: snapshot,
        contextInitialized: true,
        pendingAccountSelection: snapshot.authState === 'account_selection_required'
          ? state.pendingAccountSelection
          : null,
        employeeResources,
      };
    });
    return accepted;
  },

  refreshContext: async () => {
    try {
      const snapshot = await invoke<ManagerContextSnapshot>('manager_get_context');
      get().applyContext(snapshot);
      return snapshot;
    } catch (e) {
      const error = toManagerError(e);
      set({ identityError: error });
      throw error;
    }
  },

  clearError: () => {
    const scope = managerContextScope(get().context);
    set((state) => ({
      identityError: null,
      employeeResources: scope === null
        ? state.employeeResources
        : updateResource(state.employeeResources, scope, (resource) => ({
          ...resource,
          error: null,
        })),
    }));
  },

  dismissPaymentRequired: () => {
    const scope = managerContextScope(get().context);
    if (scope === null) return;
    set((state) => ({
      employeeResources: updateResource(state.employeeResources, scope, (resource) => ({
        ...resource,
        paymentRequired: null,
      })),
    }));
  },

  restoreSession: async () => {
    if (get().restoreAttempted || get().context.authState === 'authenticated') return null;
    set({ identityLoading: true, identityError: null, restoreAttempted: true });
    try {
      const restored = await invoke<RestoredManagerSession | null>('manager_restore_session');
      await get().refreshContext();
      if (restored) info(`manager: restored session, accountId=${restored.user.accountId}`);
      set({ identityLoading: false });
      return restored;
    } catch (e) {
      const err = toManagerError(e);
      warn(`manager: restore_session failed, kind=${err.kind}`);
      set({ identityError: err, identityLoading: false });
      return null;
    }
  },

  login: async (environment, identifier, password) => {
    set({ identityLoading: true, identityError: null, pendingAccountSelection: null });
    try {
      const result = await invoke<LoginResult>('manager_login', {
        environment,
        identifier,
        password,
      });
      await get().refreshContext();
      const contextRevision = get().context.revision;
      if (result.kind === 'account_selection_required') {
        set((state) => state.context.revision === contextRevision
          && state.context.authState === 'account_selection_required'
          ? { pendingAccountSelection: result.accounts }
          : {});
      }
      set({ identityLoading: false });
      return result;
    } catch (e) {
      const err = toManagerError(e);
      warn(`manager: login failed, kind=${err.kind}`);
      set({ identityError: err, identityLoading: false });
      throw err;
    }
  },

  selectAccount: async (accountId) => {
    set({ identityLoading: true, identityError: null });
    try {
      await invoke<UserInfo>('manager_select_account', { accountId });
      await get().refreshContext();
      set({ identityLoading: false });
    } catch (e) {
      const err = toManagerError(e);
      warn(`manager: select_account failed, kind=${err.kind}`);
      set({ identityError: err, identityLoading: false });
      throw err;
    }
  },

  fetchEmployees: async () => {
    const context = get().context;
    const scope = managerContextScope(context);
    if (scope === null) throw { kind: 'no_session' } satisfies ManagerError;
    const resource = get().employeeResources[scope] ?? createEmployeeResource(context);
    if (!resource || resource.loading) return;
    set((state) => ({
      employeeResources: {
        ...state.employeeResources,
        [scope]: { ...resource, loading: true, error: null, paymentRequired: null },
      },
    }));
    try {
      const employees = await invoke<DigitalEmployeeBrief[]>('manager_list_digital_employees');
      if (!requestStillCurrent(get(), scope)) {
        info('manager: discarded stale employee list response after Context change');
        return;
      }
      info(`manager: fetched ${employees.length} digital employees`);
      set((state) => ({
        online: true,
        employeeResources: updateResource(state.employeeResources, scope, (current) => ({
          ...current,
          employees,
          loading: false,
          lastFetchAt: Date.now(),
        })),
      }));
    } catch (e) {
      const err = toManagerError(e);
      if (!requestStillCurrent(get(), scope)) {
        info('manager: discarded stale employee list error after Context change');
        return;
      }
      warn(`manager: list_employees failed, kind=${err.kind}`);
      set((state) => ({
        online: err.kind === 'network_error' ? false : state.online,
        employeeResources: updateResource(state.employeeResources, scope, (current) => ({
          ...current,
          loading: false,
          error: err,
          paymentRequired: err.kind === 'payment_required'
            ? { message: err.detail.message, redirectUrl: err.detail.redirect_url }
            : current.paymentRequired,
        })),
      }));
      throw err;
    }
  },

  fetchEmployeesIfStale: async (maxAgeMs = EMPLOYEE_LIST_STALE_MS) => {
    const resource = currentEmployeeResource(get());
    if (!resource || resource.loading) return;
    const fresh = resource.lastFetchAt !== null
      && resource.employees.length > 0
      && Date.now() - resource.lastFetchAt < maxAgeMs;
    if (!fresh) await get().fetchEmployees();
  },

  setOnline: (online) => {
    const wasOffline = !get().online;
    set({ online });
    if (online && wasOffline && get().context.authState === 'authenticated') {
      info('manager: back online, recalibrating employee list');
      void get().fetchEmployees().catch(() => {
        /* resource error is stored */
      });
    }
  },

  selectEmployeeAndConnect: async (instanceId, employeeId) => {
    const context = get().context;
    const scope = managerContextScope(context);
    const resource = currentEmployeeResource(get());
    if (scope === null || resource === null) throw { kind: 'no_session' } satisfies ManagerError;
    if (!instanceId) {
      const err: ManagerError = { kind: 'invalid_response', detail: 'No Computer instance selected' };
      set((state) => ({
        employeeResources: updateResource(state.employeeResources, scope, (current) => ({
          ...current,
          error: err,
        })),
      }));
      throw err;
    }
    const employee = resource.employees.find((candidate) => candidate.id === employeeId);
    if (!employee) throw { kind: 'not_found' } satisfies ManagerError;
    set((state) => ({
      employeeResources: updateResource(state.employeeResources, scope, (current) => ({
        ...current,
        error: null,
        paymentRequired: null,
        selectedEmployeeId: employeeId,
        connectingEmployeeId: employeeId,
      })),
    }));
    try {
      await invoke('manager_connect_smcp', {
        instanceId,
        employeeId,
        scope: null,
      });
      if (!requestStillCurrent(get(), scope)) return null;
      await useComputerStore.getState().reconcileConnectionMetadata(instanceId);
      if (!requestStillCurrent(get(), scope)) return null;
      info(`manager: connected via token-exchange employee=${employee.name}`);
      set((state) => ({
        employeeResources: updateResource(state.employeeResources, scope, (current) => ({
          ...current,
          connectingEmployeeId: null,
        })),
      }));
      return { name: employee.name };
    } catch (e) {
      const err = toManagerError(e);
      if (!requestStillCurrent(get(), scope)) return null;
      logError(`manager: select_employee_and_connect failed, kind=${err.kind}`);
      set((state) => ({
        employeeResources: updateResource(state.employeeResources, scope, (current) => ({
          ...current,
          employees: err.kind === 'not_found_or_no_permission'
            ? current.employees.filter((candidate) => candidate.id !== employeeId)
            : current.employees,
          connectingEmployeeId: null,
          error: err,
          paymentRequired: err.kind === 'payment_required'
            ? { message: err.detail.message, redirectUrl: err.detail.redirect_url }
            : current.paymentRequired,
        })),
      }));
      if (err.kind === 'not_found_or_no_permission') {
        void get().fetchEmployees().catch(() => {
          /* resource error is stored */
        });
      }
      throw err;
    }
  },

  logout: async () => {
    set({ identityLoading: true, identityError: null });
    try {
      await invoke('manager_logout');
      await get().refreshContext();
      info('manager: logout ok');
      set({
        pendingAccountSelection: null,
        identityLoading: false,
        restoreAttempted: true,
      });
    } catch (e) {
      const err = toManagerError(e);
      set({ identityError: err, identityLoading: false });
      throw err;
    }
  },

  handleAuthExpired: async () => {
    warn('manager: auth-expired event received; reconciling authoritative Context');
    set({ identityError: { kind: 'unauthorized' } });
    try {
      await get().refreshContext();
    } catch (e) {
      warn(`manager: failed to refresh Context after auth expiry: ${String(e)}`);
    }
  },
}));
