import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info, warn, error as logError } from '@/utils/logger';
import { useConnectionStore, type ConnectionProfile } from './connectionStore';

/**
 * 登录成功后 Manager 下发的扁平 4 字段。
 * 与 Rust `services::manager_client::UserInfo` 的 serde camelCase 形态对齐。
 */
export interface UserInfo {
  userId: number;
  accountId: number;
  accountName: string;
}

/** 多账户候选项——server 实测字段结构。 */
export interface AccountOption {
  accountId: number;
  accountName: string;
  nickname: string;
  organizationId: number;
  organizationName: string;
  organizationType: string;
}

/** 数字员工列表项（`DigitalEmployeeBrief`，与 Rust DTO 对齐）。id 是数字主键。 */
export interface DigitalEmployeeBrief {
  id: number;
  name: string;
  description?: string;
  robotId?: string;
  status?: string;
  templateDisplayName?: string;
  templateType?: string;
  namespace?: string;
  clusterName?: string;
}

export interface ConnectionInfo {
  socketBaseURL: string;
  sioPath?: string;
  namespace?: string;
  rid?: string;
  robotType?: string;
  smcpNamespace?: string;
  accessToken: string;
  computerName?: string;
  routingHeaders: Record<string, string>;
  expiresAt?: string;
}

export type LoginResult =
  | { kind: 'authenticated'; user: UserInfo }
  | { kind: 'account_selection_required'; accounts: AccountOption[] };

export type ManagerError =
  | { kind: 'network_error'; detail: string }
  | { kind: 'unauthorized' }
  | { kind: 'forbidden' }
  | { kind: 'payment_required'; detail: { message: string; redirect_url?: string } }
  | { kind: 'not_found' }
  | { kind: 'other'; detail: { status: number; body: string } }
  | { kind: 'no_session' }
  | { kind: 'missing_base_url' }
  | { kind: 'invalid_response'; detail: string }
  | { kind: 'keychain_error'; detail: string };

export interface PaymentRequiredInfo {
  message: string;
  redirectUrl?: string;
}

interface ManagerState {
  baseUrl: string;
  session: UserInfo | null;
  pendingAccountSelection: AccountOption[] | null;
  employees: DigitalEmployeeBrief[];
  selectedEmployeeId: number | null;
  loading: boolean;
  error: ManagerError | null;
  paymentRequired: PaymentRequiredInfo | null;

  setBaseUrl: (url: string) => void;
  login: (phone: string, password: string, baseUrl?: string) => Promise<LoginResult>;
  selectAccount: (accountId: number) => Promise<void>;
  fetchEmployees: () => Promise<void>;
  /**
   * 选中数字员工 → 拉取 connection-info → 生成/复用 ConnectionProfile → 立即发起连接。
   * 同名 profile 的解决策略由 UI 通过 `onConflict` 传入：返回 'overwrite' / 'copy' / 'cancel'。
   */
  selectEmployeeAndConnect: (
    employeeId: number,
    onConflict?: (existingName: string) => Promise<'overwrite' | 'copy' | 'cancel'>,
  ) => Promise<{ profileName: string } | null>;
  logout: () => Promise<void>;
  handleAuthExpired: () => void;
  dismissPaymentRequired: () => void;
  clearError: () => void;
  reset: () => void;
}

const initialState = {
  baseUrl: '',
  session: null as UserInfo | null,
  pendingAccountSelection: null as AccountOption[] | null,
  employees: [] as DigitalEmployeeBrief[],
  selectedEmployeeId: null as number | null,
  loading: false,
  error: null as ManagerError | null,
  paymentRequired: null as PaymentRequiredInfo | null,
};

const TOKEN_LIKE_HEADER = /token|authorization|cookie/i;

/** Shallow-redact sensitive fields for logging. Never log access_token / Authorization. */
function redactForLog(info: ConnectionInfo) {
  const redactedHeaders: Record<string, string> = {};
  for (const [k, v] of Object.entries(info.routingHeaders || {})) {
    redactedHeaders[k] = TOKEN_LIKE_HEADER.test(k) || k === 'access_token' ? '***' : v;
  }
  return {
    socketBaseURL: info.socketBaseURL,
    sioPath: info.sioPath,
    namespace: info.namespace,
    rid: info.rid,
    robotType: info.robotType,
    smcpNamespace: info.smcpNamespace,
    computerName: info.computerName,
    routingHeaders: redactedHeaders,
    expiresAt: info.expiresAt,
  };
}

function isManagerError(e: unknown): e is ManagerError {
  return typeof e === 'object' && e !== null && typeof (e as { kind?: unknown }).kind === 'string';
}

function toManagerError(e: unknown): ManagerError {
  if (isManagerError(e)) return e;
  return { kind: 'other', detail: { status: 0, body: String(e) } };
}

function findUniqueCopyName(base: string, existing: Set<string>): string {
  for (let i = 2; i < 1000; i++) {
    const candidate = `${base} (${i})`;
    if (!existing.has(candidate)) return candidate;
  }
  return `${base} (${Date.now()})`;
}

function buildProfileFromConnectionInfo(
  employeeName: string,
  info: ConnectionInfo,
): ConnectionProfile {
  return {
    name: employeeName,
    url: info.socketBaseURL,
    namespace: info.smcpNamespace ?? '/smcp',
    office_id: info.rid ?? '',
    computer_name: info.computerName ?? '',
    headers: { ...info.routingHeaders },
    auto_connect: false,
    auto_reconnect: false,
  };
}

export const useManagerStore = create<ManagerState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  setBaseUrl: (url: string) => set({ baseUrl: url }),

  clearError: () => set({ error: null }),
  dismissPaymentRequired: () => set({ paymentRequired: null }),

  login: async (phone, password, baseUrl) => {
    set({ loading: true, error: null, paymentRequired: null });
    try {
      const effectiveBaseUrl = baseUrl ?? get().baseUrl;
      const result = await invoke<LoginResult>('manager_login', {
        baseUrl: effectiveBaseUrl || null,
        phone,
        password,
      });
      if (result.kind === 'authenticated') {
        info(`manager: login ok, accountId=${result.user.accountId}`);
        set({
          session: result.user,
          pendingAccountSelection: null,
          baseUrl: effectiveBaseUrl,
          loading: false,
        });
      } else {
        info(`manager: login requires account selection (${result.accounts.length} options)`);
        set({
          session: null,
          pendingAccountSelection: result.accounts,
          baseUrl: effectiveBaseUrl,
          loading: false,
        });
      }
      return result;
    } catch (e) {
      const err = toManagerError(e);
      warn(`manager: login failed, kind=${err.kind}`);
      set({ error: err, loading: false });
      throw err;
    }
  },

  selectAccount: async (accountId) => {
    set({ loading: true, error: null });
    try {
      const user = await invoke<UserInfo>('manager_select_account', { accountId });
      info(`manager: account selected, accountId=${user.accountId}`);
      set({ session: user, pendingAccountSelection: null, loading: false });
    } catch (e) {
      const err = toManagerError(e);
      warn(`manager: select_account failed, kind=${err.kind}`);
      set({ error: err, loading: false });
      throw err;
    }
  },

  fetchEmployees: async () => {
    // Guard against concurrent invocations. React StrictMode deliberately
    // double-fires effects in development, which otherwise produces two
    // identical IPC calls and duplicated "fetched N digital employees" logs.
    if (get().loading) return;
    set({ loading: true, error: null, paymentRequired: null });
    try {
      const employees = await invoke<DigitalEmployeeBrief[]>('manager_list_digital_employees');
      info(`manager: fetched ${employees.length} digital employees`);
      set({ employees, loading: false });
    } catch (e) {
      const err = toManagerError(e);
      warn(`manager: list_employees failed, kind=${err.kind}`);
      if (err.kind === 'payment_required') {
        set({
          paymentRequired: { message: err.detail.message, redirectUrl: err.detail.redirect_url },
        });
      }
      set({ error: err, loading: false });
      throw err;
    }
  },

  selectEmployeeAndConnect: async (employeeId, onConflict) => {
    set({ loading: true, error: null, selectedEmployeeId: employeeId, paymentRequired: null });
    const employee = get().employees.find((e) => e.id === employeeId);
    if (!employee) {
      const err: ManagerError = { kind: 'not_found' };
      set({ error: err, loading: false });
      throw err;
    }
    try {
      const infoResp = await invoke<ConnectionInfo>('manager_get_connection_info', {
        id: employeeId,
      });
      info(`manager: connection-info ok for ${employee.name}: ${JSON.stringify(redactForLog(infoResp))}`);

      await useConnectionStore.getState().fetchProfiles();
      const existingNames = new Set(
        useConnectionStore.getState().profiles.map((p) => p.name),
      );

      let profile = buildProfileFromConnectionInfo(employee.name, infoResp);
      if (existingNames.has(profile.name)) {
        const choice = onConflict ? await onConflict(profile.name) : 'overwrite';
        if (choice === 'cancel') {
          set({ loading: false });
          return null;
        }
        if (choice === 'copy') {
          profile = { ...profile, name: findUniqueCopyName(profile.name, existingNames) };
        }
      }

      await useConnectionStore.getState().saveProfile(profile);
      info(`manager: profile saved name=${profile.name}`);

      // 若当前已有 SMCP 连接，先断开再重连。
      // 原因：TFRobotServer 按 (office_id, computer_name) 识别实例，如果同 office + 同
      // computer_name 还在 room 里，server 会拒绝新连接（"Computer with name X already
      // exists in room Y"）。典型触发：用户连过一次后点"覆盖"/"另存副本"重新点连接。
      if (useConnectionStore.getState().status.connected) {
        try {
          await useConnectionStore.getState().disconnect();
          info('manager: disconnected prior session before reconnect');
        } catch (e) {
          warn(`manager: pre-connect disconnect failed, continuing anyway: ${String(e)}`);
        }
      }

      await useConnectionStore.getState().connect(profile.name);
      info(`manager: connect initiated profile=${profile.name}`);
      set({ loading: false });
      return { profileName: profile.name };
    } catch (e) {
      const err = toManagerError(e);
      logError(`manager: select_employee_and_connect failed, kind=${err.kind}`);
      if (err.kind === 'payment_required') {
        set({
          paymentRequired: { message: err.detail.message, redirectUrl: err.detail.redirect_url },
        });
      }
      set({ error: err, loading: false });
      throw err;
    }
  },

  logout: async () => {
    set({ loading: true, error: null });
    try {
      if (useConnectionStore.getState().status.connected) {
        try {
          await useConnectionStore.getState().disconnect();
        } catch (e) {
          warn(`manager: logout disconnect failed: ${String(e)}`);
        }
      }
      await invoke('manager_logout');
      info('manager: logout ok');
      set({
        session: null,
        pendingAccountSelection: null,
        employees: [],
        selectedEmployeeId: null,
        loading: false,
      });
    } catch (e) {
      const err = toManagerError(e);
      set({ error: err, loading: false });
      throw err;
    }
  },

  handleAuthExpired: () => {
    warn('manager: auth-expired event received; clearing session');
    set({
      session: null,
      pendingAccountSelection: null,
      employees: [],
      selectedEmployeeId: null,
      error: { kind: 'unauthorized' },
    });
  },
}));
