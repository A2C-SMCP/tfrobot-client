import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type {
  UserInfo,
  DigitalEmployeeBrief,
  AccountOption,
  ManagerAccountSummary,
  ManagerContextSnapshot,
  ManagerError,
} from '@/stores/managerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import i18n from '@/i18n';

type ManagerStoreMock = {
  environment: 'staging' | 'beta' | 'prod' | null;
  session: UserInfo | null;
  pendingAccountSelection: AccountOption[] | null;
  onboardingUserId: string | null;
  employees: DigitalEmployeeBrief[];
  loading: boolean;
  restoreAttempted: boolean;
  error: ManagerError | null;
  availableAccounts: ManagerAccountSummary[] | null;
  accountsLoading: boolean;
  paymentRequired: { message: string; redirectUrl?: string } | null;
  online: boolean;
  restoreSession: ReturnType<typeof vi.fn>;
  login: ReturnType<typeof vi.fn>;
  selectAccount: ReturnType<typeof vi.fn>;
  fetchAccounts: ReturnType<typeof vi.fn>;
  switchAccount: ReturnType<typeof vi.fn>;
  fetchEmployees: ReturnType<typeof vi.fn>;
  fetchEmployeesIfStale: ReturnType<typeof vi.fn>;
  setOnline: ReturnType<typeof vi.fn>;
  selectEmployeeAndConnect: ReturnType<typeof vi.fn>;
  logout: ReturnType<typeof vi.fn>;
  handleAuthExpired: ReturnType<typeof vi.fn>;
  dismissPaymentRequired: ReturnType<typeof vi.fn>;
  clearError: ReturnType<typeof vi.fn>;
  reset: ReturnType<typeof vi.fn>;
};

const mockStore: ManagerStoreMock = {
  environment: null,
  session: null,
  pendingAccountSelection: null,
  onboardingUserId: null,
  employees: [],
  loading: false,
  restoreAttempted: true,
  error: null,
  availableAccounts: null,
  accountsLoading: false,
  paymentRequired: null,
  online: true,
  restoreSession: vi.fn().mockResolvedValue(null),
  login: vi.fn().mockResolvedValue({ kind: 'authenticated' }),
  selectAccount: vi.fn().mockResolvedValue(undefined),
  fetchAccounts: vi.fn().mockResolvedValue([]),
  switchAccount: vi.fn().mockResolvedValue(undefined),
  fetchEmployees: vi.fn().mockResolvedValue(undefined),
  fetchEmployeesIfStale: vi.fn().mockResolvedValue(undefined),
  setOnline: vi.fn(),
  selectEmployeeAndConnect: vi.fn().mockResolvedValue(null),
  logout: vi.fn().mockResolvedValue(undefined),
  handleAuthExpired: vi.fn(),
  dismissPaymentRequired: vi.fn(),
  clearError: vi.fn(),
  reset: vi.fn(),
};

vi.mock('@/stores/managerStore', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/stores/managerStore')>();
  return {
    ...actual,
    useManagerStore: vi.fn(() => mockStore),
  };
});

vi.mock('@/stores/computerStore', () => ({
  useComputerStore: vi.fn(() => 'computer-a'),
}));

import { useManagerStore } from '@/stores/managerStore';
import { LoginForm } from '@/components/ManagerAccount/LoginForm';
import { AccountSelection } from '@/components/ManagerAccount/AccountSelection';
import { EmployeeList } from '@/components/ManagerAccount/EmployeeList';
import { ManagerAccount } from '@/components/ManagerAccount';
import { GlobalManagerAccount } from '@/components/ManagerAccount/GlobalManagerAccount';

const mockUseManagerStore = vi.mocked(useManagerStore);
const mockedInvoke = vi.mocked(invoke);

function adaptMock(store: ManagerStoreMock) {
  let context: ManagerContextSnapshot;
  if (store.session) {
    const organizationId = store.session.accountId.split(':')[0] || 'organization-test';
    context = {
      revision: 1,
      authState: 'authenticated',
      environment: store.environment ?? 'staging',
      contextKey: {
        environment: store.environment ?? 'staging',
        accountId: store.session.accountId,
        organizationId,
      },
      user: { id: store.session.userId, nickname: 'User', email: '', phone: '' },
      account: {
        id: store.session.accountId,
        name: store.session.accountName,
        nickname: 'User',
        avatar: '',
        employeeNo: '',
      },
      organization: { id: organizationId, name: 'Organization', organizationType: 'team' },
      permissions: [],
    };
  } else if (store.onboardingUserId !== null) {
    context = {
      revision: 1,
      authState: 'onboarding_required',
      environment: store.environment ?? 'staging',
      contextKey: null,
      user: { id: store.onboardingUserId, nickname: '', email: '', phone: '' },
      account: null,
      organization: null,
      permissions: [],
    };
  } else {
    context = {
      revision: store.pendingAccountSelection ? 1 : 0,
      authState: store.pendingAccountSelection ? 'account_selection_required' : 'signed_out',
      environment: store.pendingAccountSelection ? store.environment ?? 'staging' : null,
      contextKey: null,
      user: null,
      account: null,
      organization: null,
      permissions: [],
    };
  }
  const scope = context.contextKey
    ? JSON.stringify([
      context.contextKey.environment,
      context.contextKey.accountId,
      context.contextKey.organizationId,
      context.revision,
    ])
    : null;
  return {
    ...store,
    context,
    contextInitialized: true,
    identityLoading: store.loading,
    identityError: store.error,
    accountDirectoryScope: scope,
    employeeResources: scope
      ? {
        [scope]: {
          scope,
          contextKey: context.contextKey!,
          revision: context.revision,
          employees: store.employees,
          loading: store.loading,
          error: store.error,
          paymentRequired: store.paymentRequired,
          lastFetchAt: Date.now(),
          selectedEmployeeId: null,
          connectingEmployeeId: null,
        },
      }
      : {},
  };
}

function applyMock(overrides: Partial<ManagerStoreMock> = {}) {
  mockUseManagerStore.mockReturnValue(adaptMock({ ...mockStore, ...overrides }) as ReturnType<
    typeof useManagerStore
  >);
}

describe('ManagerAccount', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedInvoke.mockResolvedValue({ connected: false });
    // connectionStore 是真实 store（未 mock），复位到断开状态，让 EmployeeList 默认渲染"连接"而非"已连接"
    useConnectionStore.setState({
      statuses: {},
      loading: false,
      error: null,
    });
    applyMock();
  });

  describe('LoginForm', () => {
    it('renders the sign-in heading and environment hint', () => {
      render(<LoginForm />);
      expect(screen.getByText('Sign in to TFRSManager')).toBeInTheDocument();
      expect(
        screen.getByText('The client configures the Manager address for the selected environment.'),
      ).toBeInTheDocument();
    });

    it('calls login with form values', async () => {
      const login = vi.fn().mockResolvedValue({ kind: 'authenticated' });
      applyMock({ login });

      render(<LoginForm />);
      fireEvent.change(screen.getByLabelText('Phone or email'), {
        target: { value: '13800138008' },
      });
      fireEvent.change(screen.getByLabelText('Password'), {
        target: { value: 'Test@123456' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Sign In/i }));

      await waitFor(() => {
        expect(login).toHaveBeenCalledWith('staging', '13800138008', 'Test@123456');
      });
    });

    it('renders localized error message', () => {
      applyMock({ error: { kind: 'unauthorized' } });
      render(<LoginForm />);
      expect(
        screen.getByText('Your Manager session has expired. Please sign in again.'),
      ).toBeInTheDocument();
    });

    it('renders missing_base_url hint', () => {
      applyMock({ error: { kind: 'missing_base_url' } });
      render(<LoginForm />);
      expect(
        screen.getByText(
          'The Manager environment could not be resolved. Select an environment again.',
        ),
      ).toBeInTheDocument();
    });
  });

  describe('GlobalManagerAccount', () => {
    it('opens an app-wide sign-in dialog and restores focus with Escape', async () => {
      render(<GlobalManagerAccount />);
      const trigger = screen.getByRole('button', { name: /Sign In/i });

      fireEvent.click(trigger);
      const dialog = await screen.findByRole('dialog', { name: 'Manager Account' });
      expect(screen.getByText('Sign in to TFRSManager')).toBeInTheDocument();

      fireEvent.keyDown(dialog, { key: 'Escape' });
      await waitFor(() => expect(trigger).toHaveAttribute('aria-expanded', 'false'));
      await waitFor(() => expect(trigger).toHaveFocus());
    });

    it('shows current Context and switches only through the store transaction action', async () => {
      const accounts: ManagerAccountSummary[] = [
        {
          accountId: 'org-legacy-9:account-16',
          accountName: 'client_uat',
          nickname: 'Current Account',
          organizationId: 'org-legacy-9',
          organizationName: 'Organization',
          organizationType: 'enterprise',
          role: 'owner',
        },
        {
          accountId: '42',
          accountName: 'other_account',
          nickname: 'Other Account',
          organizationId: '84',
          organizationName: 'Other Organization',
          organizationType: 'enterprise',
          role: 'member',
        },
      ];
      const fetchAccounts = vi.fn().mockResolvedValue(accounts);
      const switchAccount = vi.fn().mockResolvedValue(undefined);
      applyMock({
        session: {
          userId: '9',
          accountId: 'org-legacy-9:account-16',
          accountName: 'client_uat',
        },
        availableAccounts: accounts,
        fetchAccounts,
        switchAccount,
      });

      render(<GlobalManagerAccount />);
      fireEvent.click(screen.getByRole('button', { name: /Organization, User/i }));
      await screen.findByRole('dialog', { name: 'Manager Account' });
      expect(screen.getByText('Staging')).toBeInTheDocument();
      expect(screen.getByText('client_uat')).toBeInTheDocument();
      expect(fetchAccounts).toHaveBeenCalledOnce();

      fireEvent.click(screen.getByRole('button', { name: /Other Account/i }));
      await waitFor(() => expect(switchAccount).toHaveBeenCalledWith('42'));
    });

    it('offers one clear sign-out action without an ineffective re-login path', async () => {
      const logout = vi.fn().mockResolvedValue(undefined);
      applyMock({
        session: {
          userId: '9',
          accountId: '16',
          accountName: 'client_uat',
        },
        availableAccounts: [],
        logout,
      });

      render(<GlobalManagerAccount />);
      fireEvent.click(screen.getByRole('button', { name: /Organization, User/i }));
      await screen.findByRole('dialog', { name: 'Manager Account' });

      expect(screen.queryByRole('button', { name: /Sign in again/i })).not.toBeInTheDocument();
      const signOut = screen.getByRole('button', { name: /Sign Out/i });
      fireEvent.click(signOut);
      await waitFor(() => expect(logout).toHaveBeenCalledOnce());
    });

    it('surfaces pending account selection from every page entry', async () => {
      applyMock({
        pendingAccountSelection: [{
          accountId: '42',
          accountName: 'Account 42',
          nickname: 'Account 42',
          organizationId: '84',
          organizationName: 'Organization 84',
          organizationType: 'enterprise',
        }],
      });

      render(<GlobalManagerAccount />);
      fireEvent.click(screen.getByRole('button', { name: /Complete sign-in/i }));
      await screen.findByRole('dialog', { name: 'Manager Account' });
      expect(screen.getByText('Choose an account')).toBeInTheDocument();
    });

    it('loads the account directory when sign-in completes while the panel stays open', async () => {
      const fetchAccounts = vi.fn().mockResolvedValue([]);
      const view = render(<GlobalManagerAccount />);
      fireEvent.click(screen.getByRole('button', { name: /Sign In/i }));
      await screen.findByRole('dialog', { name: 'Manager Account' });
      expect(fetchAccounts).not.toHaveBeenCalled();

      applyMock({
        session: {
          userId: '9',
          accountId: '16',
          accountName: 'client_uat',
        },
        fetchAccounts,
      });
      view.rerender(<GlobalManagerAccount />);

      await waitFor(() => expect(fetchAccounts).toHaveBeenCalledOnce());
      expect(screen.getByText('No other accounts are available.')).toBeInTheDocument();
    });
  });

  describe('AccountSelection', () => {
    it('lists accounts and triggers selectAccount', async () => {
      const selectAccount = vi.fn().mockResolvedValue(undefined);
      applyMock({
        pendingAccountSelection: [
          {
            accountId: 'org-2:account-2',
            accountName: 'testuser2_enterprise',
            nickname: '测试企业主账号',
            organizationId: 'org-2',
            organizationName: '测试企业',
            organizationType: 'enterprise',
          },
          {
            accountId: 'org-1:account-3',
            accountName: 'testuser2_personal',
            nickname: '个人账号',
            organizationId: 'org-1',
            organizationName: 'one-person-org-1',
            organizationType: 'personal',
          },
        ],
        selectAccount,
      });

      render(<AccountSelection />);
      expect(screen.getByText('测试企业主账号')).toBeInTheDocument();
      expect(screen.getByText('testuser2_enterprise')).toBeInTheDocument();
      expect(screen.getByText('个人账号')).toBeInTheDocument();

      fireEvent.click(screen.getAllByRole('button', { name: 'Select' })[0]);
      await waitFor(() => {
        expect(selectAccount).toHaveBeenCalledWith('org-2:account-2');
      });
    }, 10000);
  });

  describe('EmployeeList', () => {
    const user: UserInfo = {
      userId: '9',
      accountId: 'org-legacy-9:account-16',
      accountName: 'client_uat',
    };
    const employee: DigitalEmployeeBrief = {
      id: 11,
      name: 'bot-one',
      robotId: 'robot-a',
      robotAccountId: 'turingfocus:004242',
      templateType: 'tfrserver',
      templateDisplayName: '智能客服',
      status: 'running',
    };

    it('fetches employees on mount (staleness-gated) and renders them', async () => {
      const fetchEmployeesIfStale = vi.fn().mockResolvedValue(undefined);
      applyMock({ session: user, employees: [employee], fetchEmployeesIfStale });

      render(<EmployeeList instanceId="computer-a" />);
      await waitFor(() => expect(fetchEmployeesIfStale).toHaveBeenCalled());
      expect(screen.getByText('bot-one')).toBeInTheDocument();
      expect(screen.getByText('robot-a')).toBeInTheDocument();
    });

    it('localizes employee status labels in Chinese', async () => {
      await i18n.changeLanguage('zh');
      try {
        applyMock({
          session: user,
          employees: [employee, { ...employee, id: 12, name: 'bot-two', status: 'stop_failed' }],
        });

        render(<EmployeeList />);

        expect(screen.getByText('运行中')).toBeInTheDocument();
        expect(screen.getByText('停止失败')).toBeInTheDocument();
        expect(screen.queryByText('running')).not.toBeInTheDocument();
        expect(screen.queryByText('stop_failed')).not.toBeInTheDocument();
      } finally {
        await i18n.changeLanguage('en');
      }
    });

    it('hides the identity summary while keeping refresh and employee content', () => {
      applyMock({ session: user, employees: [employee] });

      render(
        <EmployeeList showIdentityActions={false} showIdentitySummary={false} />,
      );

      expect(screen.queryByText('Digital Employees')).not.toBeInTheDocument();
      expect(screen.queryByText(`Signed in as ${user.accountName}`)).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Refresh/i })).toBeInTheDocument();
      expect(screen.getByText('bot-one')).toBeInTheDocument();
    });

    it('shows empty state when no employees', async () => {
      applyMock({ session: user, employees: [] });
      render(<EmployeeList instanceId="computer-a" />);
      expect(
        screen.getByText('No digital employees are available for this account.'),
      ).toBeInTheDocument();
    });

    it('does not refetch an empty employee list when the authenticated scope is unchanged', async () => {
      const fetchEmployeesIfStale = vi.fn().mockResolvedValue(undefined);
      applyMock({ session: user, employees: [], fetchEmployeesIfStale });
      const view = render(<EmployeeList instanceId="computer-a" />);
      await waitFor(() => expect(fetchEmployeesIfStale).toHaveBeenCalledOnce());

      view.rerender(<EmployeeList instanceId="computer-a" />);

      await waitFor(() => expect(fetchEmployeesIfStale).toHaveBeenCalledOnce());
    });

    it('renders Manager robots as resources without connection actions when no Computer is scoped', () => {
      applyMock({ session: user, employees: [employee] });

      render(<EmployeeList />);

      expect(screen.getByText('bot-one')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /Connect/i })).not.toBeInTheDocument();
      expect(mockedInvoke).not.toHaveBeenCalledWith('get_connection_status', expect.anything());
    });

    it('invokes selectEmployeeAndConnect on connect click', async () => {
      const selectEmployeeAndConnect = vi.fn().mockResolvedValue({ name: 'bot-one' });
      applyMock({ session: user, employees: [employee], selectEmployeeAndConnect });
      useConnectionStore.setState({
        statuses: {
          'computer-a': {
            status: 'disconnected',
            connected: false,
            actions: {
              connect: { enabled: true, disabled_reason: null },
              disconnect: { enabled: false, disabled_reason: 'not_connected' },
            },
          },
        },
      });
      mockedInvoke.mockImplementationOnce(() => new Promise(() => {}));

      render(<EmployeeList instanceId="computer-a" />);
      fireEvent.click(screen.getByRole('button', { name: /Connect/i }));
      await waitFor(() => {
        expect(selectEmployeeAndConnect).toHaveBeenCalled();
        expect(selectEmployeeAndConnect.mock.calls[0]).toEqual(['computer-a', 11]);
      });
    });

    it('keeps connect disabled until backend capabilities are hydrated', () => {
      applyMock({ session: user, employees: [employee] });

      render(<EmployeeList instanceId="computer-a" />);

      expect(screen.getByText('Connect').closest('button')).toBeDisabled();
    });

    it('shows disconnect for manager connection even when robotId is missing', async () => {
      const connectedWithoutRobotId = { ...employee, robotId: undefined };
      applyMock({ session: user, employees: [connectedWithoutRobotId] });
      mockedInvoke.mockResolvedValueOnce({
        connected: true,
        office_id: 'server-rid',
        profile_name: 'manager:11',
      });
      useConnectionStore.setState({
        statuses: {
          'computer-a': {
            connected: true,
            office_id: 'server-rid',
            profile_name: 'manager:11',
          },
        },
      });

      render(<EmployeeList instanceId="computer-a" />);
      expect(screen.getByRole('button', { name: /Disconnect/i })).toBeInTheDocument();
    });

    it('uses the backend operation target for Manager connect transitions', () => {
      const otherEmployee = {
        ...employee,
        id: 12,
        name: 'bot-two',
        robotId: 'robot-b',
        robotAccountId: 'turingfocus:004343',
      };
      applyMock({
        session: user,
        employees: [employee, otherEmployee],
        loading: false,
      });
      useConnectionStore.setState({
        statuses: {
          'computer-a': {
            status: 'connecting',
            connected: false,
            operation: 'connect',
            operation_target: {
              source_type: 'manager_robot',
              target_id: `manager:${employee.id}`,
              employee_id: employee.id,
            },
            actions: {
              connect: { enabled: false, disabled_reason: 'transition_in_progress' },
              disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
            },
          },
        },
      });

      render(<EmployeeList instanceId="computer-a" />);
      const connectButtons = screen.getAllByRole('button', { name: /Connect/i });
      expect(connectButtons).toHaveLength(2);
      expect(connectButtons[0]).toBeDisabled();
      expect(connectButtons[0]).toHaveClass('ant-btn-loading');
      expect(connectButtons[1]).toBeDisabled();
      expect(connectButtons[1]).not.toHaveClass('ant-btn-loading');
    });

    it('keeps the backend disconnect action visible while disconnecting', () => {
      applyMock({ session: user, employees: [employee], loading: false });
      useConnectionStore.setState({
        statuses: {
          'computer-a': {
            status: 'disconnecting',
            connected: false,
            operation: 'disconnect',
            profile_name: 'manager:11',
            office_id: 'robot-a',
            actions: {
              connect: { enabled: false, disabled_reason: 'transition_in_progress' },
              disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
            },
          },
        },
      });

      render(<EmployeeList instanceId="computer-a" />);
      const disconnect = screen.getByRole('button', { name: /Disconnect/i });
      expect(disconnect).toBeDisabled();
      expect(disconnect).toHaveClass('ant-btn-loading');
      expect(screen.queryByText('Connect')).not.toBeInTheDocument();
    });

    it('offers Computer-level cleanup when an orphan transport remains', () => {
      applyMock({ session: user, employees: [employee], loading: false });
      useConnectionStore.setState({
        statuses: {
          'computer-a': {
            status: 'disconnected',
            connected: false,
            operation: undefined,
            last_error: {
              operation: 'disconnect',
              message: 'Socket cleanup failed',
              retryable: true,
              occurred_at: '2026-07-29T02:00:00Z',
            },
            actions: {
              connect: { enabled: false, disabled_reason: 'connection_unavailable' },
              disconnect: { enabled: true, disabled_reason: null },
            },
          },
        },
      });

      render(<EmployeeList instanceId="computer-a" />);

      expect(screen.getByText(
        'A stale connection must be cleaned up before reconnecting.',
      )).toBeInTheDocument();
      expect(screen.getByText(
        'Retry the disconnect action. If cleanup still fails, view Runtime diagnostics or logs.',
      )).toBeInTheDocument();
      expect(screen.queryByText('Socket cleanup failed')).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Disconnect/i })).toBeEnabled();
      expect(screen.getByText('Connect').closest('button')).toBeDisabled();
    });

    it('does not expose a technical error when orphan cleanup fails', async () => {
      applyMock({ session: user, employees: [employee], loading: false });
      useConnectionStore.setState({
        statuses: {
          'computer-a': {
            status: 'disconnected',
            connected: false,
            actions: {
              connect: { enabled: false, disabled_reason: 'connection_unavailable' },
              disconnect: { enabled: true, disabled_reason: null },
            },
          },
        },
      });
      mockedInvoke.mockReset();
      mockedInvoke.mockRejectedValueOnce(
        new Error('cleanup https://private.example failed with token=private'),
      );

      render(<EmployeeList instanceId="computer-a" />);
      fireEvent.click(screen.getByRole('button', { name: /Disconnect/i }));

      expect(await screen.findByText(
        'The connection operation failed. View Runtime diagnostics or logs for details.',
      )).toBeInTheDocument();
      expect(screen.queryByText(/private\.example|token=private/)).not.toBeInTheDocument();
    }, 10000);

    it('does not expose Manager connection details in the ordinary error alert', () => {
      applyMock({
        session: user,
        employees: [employee],
        error: {
          kind: 'network_error',
          detail: 'https://private.example failed with token=private',
        },
      });

      render(<EmployeeList instanceId="computer-a" />);

      expect(screen.getByText(
        'Cannot reach the Manager server. Check your connection or selected environment.',
      )).toBeInTheDocument();
      expect(screen.queryByText(/private\.example|token=private/)).not.toBeInTheDocument();
    });

    it('disables connect button for non-running status', async () => {
      applyMock({
        session: user,
        employees: [{ ...employee, status: 'suspended' }],
      });
      render(<EmployeeList instanceId="computer-a" />);
      expect(screen.getByRole('button', { name: /Connect/i })).toBeDisabled();
    }, 10000);

    it('allows connect when cached robotAccountId is missing so backend can re-resolve it', async () => {
      const noAccount = { ...employee, robotAccountId: undefined };
      applyMock({ session: user, employees: [noAccount] });
      useConnectionStore.setState({
        statuses: {
          'computer-a': {
            status: 'disconnected',
            connected: false,
            actions: {
              connect: { enabled: true, disabled_reason: null },
              disconnect: { enabled: false, disabled_reason: 'not_connected' },
            },
          },
        },
      });
      render(<EmployeeList instanceId="computer-a" />);
      expect(screen.getByRole('button', { name: /Connect/i })).toBeEnabled();
    }, 10000);

    it('renders payment_required alert with renew button when redirectUrl present', async () => {
      applyMock({
        session: user,
        employees: [employee],
        paymentRequired: { message: 'Balance low', redirectUrl: 'https://pay.example.com' },
      });
      render(<EmployeeList instanceId="computer-a" />);
      expect(screen.getByText('Balance low')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Renew subscription' })).toBeInTheDocument();
    }, 10000);
  });

  describe('Router (ManagerAccount)', () => {
    it('renders LoginForm when no session', () => {
      render(<ManagerAccount />);
      expect(screen.getByText('Sign in to TFRSManager')).toBeInTheDocument();
    });

    it('renders AccountSelection when pendingAccountSelection is set', () => {
      applyMock({
        pendingAccountSelection: [
          {
            accountId: 'org-2:account-2',
            accountName: 'Acct One',
            nickname: 'Acct One',
            organizationId: 'org-2',
            organizationName: 'Org',
            organizationType: 'enterprise',
          },
        ],
      });
      render(<ManagerAccount />);
      expect(screen.getByText('Choose an account')).toBeInTheDocument();
    });

    it('renders EmployeeList when session is present', async () => {
      applyMock({
        session: {
          userId: '9',
          accountId: 'org-legacy-9:account-16',
          accountName: 'client_uat',
        },
        employees: [],
      });
      render(<ManagerAccount />);
      expect(screen.getByText('Digital Employees')).toBeInTheDocument();
    });

    it('renders onboarding guidance without creating an organization in the client', () => {
      applyMock({ onboardingUserId: '99' });

      render(<ManagerAccount />);

      expect(screen.getByText('No organization account yet')).toBeInTheDocument();
      expect(
        screen.getByText(/Complete organization setup or join an organization in TFRS FrontPortal/),
      ).toBeInTheDocument();
    });
  });
});
