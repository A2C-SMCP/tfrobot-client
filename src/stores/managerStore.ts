import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info, warn, error as logError } from '@/utils/logger';
import { useConnectionStore } from './connectionStore';
import { useComputerStore } from './computerStore';

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

/** 部门祖先链元素（`departments[].ancestors[]`）。根→叶有序，末元素即本部门。 */
export interface DepartmentAncestor {
  id: number;
  name: string;
}

/**
 * 部门归属（`departments[]` 元素，与 Rust `DepartmentRef` 对齐）。
 * `ancestors` 含自身、根→叶有序；按序 join `name` 即面包屑。不受可见性 flag 控制。
 */
export interface DepartmentRef {
  id: number;
  name: string;
  path?: string;
  ancestors?: DepartmentAncestor[];
}

/** 数字员工列表项（`DigitalEmployeeBrief`，与 Rust DTO 对齐）。id 是数字主键。 */
export interface DigitalEmployeeBrief {
  id: number;
  name: string;
  description?: string;
  robotId?: string;
  /**
   * 机器人自身账号 ID（TFRM-183，nullable）。token-exchange 的 audience = `robot:<robotAccountId>`。
   * 为空表示历史/未回填实例——无法走安全连接，UI 应禁用其连接按钮。
   * 注意与 `robotId`（SMCP 路由串）区分。
   */
  robotAccountId?: number;
  status?: string;
  templateDisplayName?: string;
  templateType?: string;
  namespace?: string;
  clusterName?: string;
  /** 部门归属（TFRM-56）。后端保证为数组（可能为空 []）。 */
  departments?: DepartmentRef[];
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
  | { kind: 'not_found_or_no_permission' }
  | { kind: 'other'; detail: { status: number; body: string } }
  | { kind: 'no_session' }
  | { kind: 'missing_base_url' }
  | { kind: 'invalid_response'; detail: string }
  | { kind: 'keychain_error'; detail: string }
  // TFRC-11 token-exchange：RFC 6749 §5.2 失败 / 签名子系统未就位。
  | { kind: 'token_exchange'; detail: { error: string; description?: string } }
  | { kind: 'signing_unavailable'; detail: { message?: string } };

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
  /** 上次成功拉取员工列表的时间戳（ms）。null = 尚未成功拉过。用于 60s staleness 兜底。 */
  lastFetchAt: number | null;
  /** 在线状态。false 时 UI 展示离线横幅并保留最近一次成功列表（TFRM-56 离线模式）。 */
  online: boolean;

  setBaseUrl: (url: string) => void;
  login: (phone: string, password: string, baseUrl?: string) => Promise<LoginResult>;
  selectAccount: (accountId: number) => Promise<void>;
  fetchEmployees: () => Promise<void>;
  /**
   * 进入列表页时的兜底拉取：仅当无数据、或距上次成功拉取已超过 `maxAgeMs`（默认 60s，
   * 与后端可见集合缓存 TTL 对齐）时才真正请求；否则复用当前列表（TFRM-56）。
   */
  fetchEmployeesIfStale: (maxAgeMs?: number) => Promise<void>;
  /**
   * 同步在线状态。离线 → 在线的跳变会立即触发一次校准 refetch（不受 staleness 窗口限制）。
   */
  setOnline: (online: boolean) => void;
  /**
   * 选中数字员工 → 后端编排 token-exchange 全路径并连接（TFRC-11 / C1）：
   * `connection-info → exchange_token(robotAccountId) → 短 JWT 注入 Socket.IO auth dict → 连接`，
   * 并起后台预刷新重连。鉴权不再走静态 token / profile。
   * 返回 `{ name }`（已连接的机器人名）或 `null`（前置校验未过）。
   */
  selectEmployeeAndConnect: (employeeId: number) => Promise<{ name: string } | null>;
  logout: () => Promise<void>;
  handleAuthExpired: () => void;
  dismissPaymentRequired: () => void;
  clearError: () => void;
  reset: () => void;
}

/** 列表 staleness 窗口，与后端可见集合缓存 TTL 对齐（TFRM-167 评论：60s）。 */
const EMPLOYEE_LIST_STALE_MS = 60_000;

const initialState = {
  baseUrl: '',
  session: null as UserInfo | null,
  pendingAccountSelection: null as AccountOption[] | null,
  employees: [] as DigitalEmployeeBrief[],
  selectedEmployeeId: null as number | null,
  loading: false,
  error: null as ManagerError | null,
  paymentRequired: null as PaymentRequiredInfo | null,
  lastFetchAt: null as number | null,
  online: true,
};

function isManagerError(e: unknown): e is ManagerError {
  return typeof e === 'object' && e !== null && typeof (e as { kind?: unknown }).kind === 'string';
}

function toManagerError(e: unknown): ManagerError {
  if (isManagerError(e)) return e;
  return { kind: 'other', detail: { status: 0, body: String(e) } };
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
      set({ employees, loading: false, lastFetchAt: Date.now(), online: true });
    } catch (e) {
      const err = toManagerError(e);
      warn(`manager: list_employees failed, kind=${err.kind}`);
      if (err.kind === 'payment_required') {
        set({
          paymentRequired: { message: err.detail.message, redirectUrl: err.detail.redirect_url },
        });
      }
      // 网络失败：保留最近一次成功列表（不清空 employees），标记离线供 UI 横幅展示。
      set({
        error: err,
        loading: false,
        online: err.kind === 'network_error' ? false : get().online,
      });
      throw err;
    }
  },

  fetchEmployeesIfStale: async (maxAgeMs = EMPLOYEE_LIST_STALE_MS) => {
    const { lastFetchAt, employees, loading } = get();
    if (loading) return;
    const fresh =
      lastFetchAt !== null && employees.length > 0 && Date.now() - lastFetchAt < maxAgeMs;
    if (fresh) return;
    await get().fetchEmployees();
  },

  setOnline: (online: boolean) => {
    const wasOffline = !get().online;
    set({ online });
    // 离线 → 在线跳变：立即校准（绕过 staleness 窗口），让被剔除/新增的项即时对齐。
    if (online && wasOffline && get().session) {
      info('manager: back online, recalibrating employee list');
      get()
        .fetchEmployees()
        .catch(() => {
          /* error stored in store */
        });
    }
  },

  selectEmployeeAndConnect: async (employeeId) => {
    set({ loading: true, error: null, selectedEmployeeId: employeeId, paymentRequired: null });
    const employee = get().employees.find((e) => e.id === employeeId);
    if (!employee) {
      const err: ManagerError = { kind: 'not_found' };
      set({ error: err, loading: false });
      throw err;
    }
    // 安全连接需要 robotAccountId 作 token-exchange audience（TFRM-183，nullable）。
    // 历史/未回填实例无此 ID，无法换取连接凭据。
    if (employee.robotAccountId == null) {
      const err: ManagerError = {
        kind: 'invalid_response',
        detail: 'robotAccountId missing for this robot',
      };
      set({ error: err, loading: false });
      throw err;
    }
    const instanceId = useComputerStore.getState().selectedInstanceId;
    if (!instanceId) {
      const err: ManagerError = {
        kind: 'invalid_response',
        detail: 'No Computer instance selected',
      };
      set({ error: err, loading: false });
      throw err;
    }
    try {
      // 后端编排 token-exchange 全路径：connection-info → exchange_token → 短 JWT 注入 Socket.IO
      // auth dict → 连接，并起后台预刷新重连。鉴权不再走静态 token / profile。
      await invoke('manager_connect_smcp', {
        instanceId,
        employeeId,
        robotAccountId: employee.robotAccountId,
        scope: null,
      });
      info(`manager: connected via token-exchange employee=${employee.name}`);
      set({ loading: false });
      return { name: employee.name };
    } catch (e) {
      const err = toManagerError(e);
      logError(`manager: select_employee_and_connect failed, kind=${err.kind}`);
      if (err.kind === 'payment_required') {
        set({
          paymentRequired: { message: err.detail.message, redirectUrl: err.detail.redirect_url },
        });
      }
      if (err.kind === 'not_found_or_no_permission') {
        // 该机器人对当前 viewer 已不可见（部门可见性回收）：本地剔除该项。
        warn(`manager: employee ${employeeId} no longer visible, removing from local list`);
        set({ employees: get().employees.filter((e2) => e2.id !== employeeId) });
      }
      set({ error: err, loading: false });
      // loading 已置 false，可安全触发校准 refetch（绕过并发 guard）。
      if (err.kind === 'not_found_or_no_permission') {
        get()
          .fetchEmployees()
          .catch(() => {
            /* error stored in store */
          });
      }
      throw err;
    }
  },

  logout: async () => {
    set({ loading: true, error: null });
    try {
      if (useConnectionStore.getState().status.connected) {
        try {
          const instanceId = useComputerStore.getState().selectedInstanceId;
          if (instanceId) {
            await useConnectionStore.getState().disconnect(instanceId);
          }
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
        lastFetchAt: null,
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
      lastFetchAt: null,
      error: { kind: 'unauthorized' },
    });
  },
}));
