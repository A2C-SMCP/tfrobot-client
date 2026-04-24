import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import type {
  UserInfo,
  DigitalEmployeeBrief,
  AccountOption,
  ManagerError,
} from '@/stores/managerStore';
import { useConnectionStore } from '@/stores/connectionStore';

type ManagerStoreMock = {
  baseUrl: string;
  session: UserInfo | null;
  pendingAccountSelection: AccountOption[] | null;
  employees: DigitalEmployeeBrief[];
  selectedEmployeeId: number | null;
  loading: boolean;
  error: ManagerError | null;
  paymentRequired: { message: string; redirectUrl?: string } | null;
  setBaseUrl: ReturnType<typeof vi.fn>;
  login: ReturnType<typeof vi.fn>;
  selectAccount: ReturnType<typeof vi.fn>;
  fetchEmployees: ReturnType<typeof vi.fn>;
  selectEmployeeAndConnect: ReturnType<typeof vi.fn>;
  logout: ReturnType<typeof vi.fn>;
  handleAuthExpired: ReturnType<typeof vi.fn>;
  dismissPaymentRequired: ReturnType<typeof vi.fn>;
  clearError: ReturnType<typeof vi.fn>;
  reset: ReturnType<typeof vi.fn>;
};

const mockStore: ManagerStoreMock = {
  baseUrl: '',
  session: null,
  pendingAccountSelection: null,
  employees: [],
  selectedEmployeeId: null,
  loading: false,
  error: null,
  paymentRequired: null,
  setBaseUrl: vi.fn(),
  login: vi.fn().mockResolvedValue({ kind: 'authenticated' }),
  selectAccount: vi.fn().mockResolvedValue(undefined),
  fetchEmployees: vi.fn().mockResolvedValue(undefined),
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

import { useManagerStore } from '@/stores/managerStore';
import { LoginForm } from '@/components/ManagerAccount/LoginForm';
import { AccountSelection } from '@/components/ManagerAccount/AccountSelection';
import { EmployeeList } from '@/components/ManagerAccount/EmployeeList';
import { ManagerAccount } from '@/components/ManagerAccount';

const mockUseManagerStore = vi.mocked(useManagerStore);

function applyMock(overrides: Partial<ManagerStoreMock> = {}) {
  mockUseManagerStore.mockReturnValue({ ...mockStore, ...overrides } as ReturnType<
    typeof useManagerStore
  >);
}

describe('ManagerAccount', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // connectionStore 是真实 store（未 mock），复位到断开状态，让 EmployeeList 默认渲染"连接"而非"已连接"
    useConnectionStore.setState({
      profiles: [],
      status: { connected: false },
      loading: false,
      error: null,
    });
    applyMock();
  });

  describe('LoginForm', () => {
    it('renders the sign-in heading and baseUrl hint', () => {
      render(<LoginForm />);
      expect(screen.getByText('Sign in to TFRSManager')).toBeInTheDocument();
      expect(
        screen.getByText('Leave empty to use the TFRS_MANAGER_BASE_URL environment variable.'),
      ).toBeInTheDocument();
    });

    it('calls login with form values', async () => {
      const login = vi.fn().mockResolvedValue({ kind: 'authenticated' });
      applyMock({ login });

      render(<LoginForm />);
      fireEvent.change(screen.getByLabelText('Manager Base URL'), {
        target: { value: 'http://localhost:8090' },
      });
      fireEvent.change(screen.getByLabelText('Phone'), {
        target: { value: '13800138008' },
      });
      fireEvent.change(screen.getByLabelText('Password'), {
        target: { value: 'Test@123456' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Sign In/i }));

      await waitFor(() => {
        expect(login).toHaveBeenCalledWith(
          '13800138008',
          'Test@123456',
          'http://localhost:8090',
        );
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
          'The Manager base URL is not configured. Set TFRS_MANAGER_BASE_URL or enter it on the sign-in form.',
        ),
      ).toBeInTheDocument();
    });
  });

  describe('AccountSelection', () => {
    it('lists accounts and triggers selectAccount', async () => {
      const selectAccount = vi.fn().mockResolvedValue(undefined);
      applyMock({
        pendingAccountSelection: [
          {
            accountId: 2,
            accountName: 'testuser2_enterprise',
            nickname: '测试企业主账号',
            organizationId: 2,
            organizationName: '测试企业',
            organizationType: 'enterprise',
          },
          {
            accountId: 3,
            accountName: 'testuser2_personal',
            nickname: '个人账号',
            organizationId: 1,
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
        expect(selectAccount).toHaveBeenCalledWith(2);
      });
    });
  });

  describe('EmployeeList', () => {
    const user: UserInfo = { userId: 9, accountId: 16, accountName: 'client_uat' };
    const employee: DigitalEmployeeBrief = {
      id: 11,
      name: 'bot-one',
      robotId: 'robot-a',
      templateType: 'tfrserver',
      templateDisplayName: '智能客服',
      status: 'running',
    };

    it('fetches employees on mount and renders them', async () => {
      const fetchEmployees = vi.fn().mockResolvedValue(undefined);
      applyMock({ session: user, employees: [employee], fetchEmployees });

      render(<EmployeeList />);
      expect(fetchEmployees).toHaveBeenCalled();
      expect(screen.getByText('bot-one')).toBeInTheDocument();
      expect(screen.getByText('robot-a')).toBeInTheDocument();
    });

    it('shows empty state when no employees', () => {
      applyMock({ session: user, employees: [] });
      render(<EmployeeList />);
      expect(
        screen.getByText('No digital employees are available for this account.'),
      ).toBeInTheDocument();
    });

    it('invokes selectEmployeeAndConnect on connect click', async () => {
      const selectEmployeeAndConnect = vi
        .fn()
        .mockResolvedValue({ profileName: 'bot-one' });
      applyMock({ session: user, employees: [employee], selectEmployeeAndConnect });

      render(<EmployeeList />);
      fireEvent.click(screen.getByRole('button', { name: /Connect/i }));
      await waitFor(() => {
        expect(selectEmployeeAndConnect).toHaveBeenCalled();
        expect(selectEmployeeAndConnect.mock.calls[0][0]).toBe(11);
      });
    });

    it('disables connect button for non-running status', () => {
      applyMock({
        session: user,
        employees: [{ ...employee, status: 'suspended' }],
      });
      render(<EmployeeList />);
      expect(screen.getByRole('button', { name: /Connect/i })).toBeDisabled();
    });

    it('renders payment_required alert with renew button when redirectUrl present', () => {
      applyMock({
        session: user,
        employees: [employee],
        paymentRequired: { message: 'Balance low', redirectUrl: 'https://pay.example.com' },
      });
      render(<EmployeeList />);
      expect(screen.getByText('Balance low')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Renew subscription' })).toBeInTheDocument();
    });
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
            accountId: 2,
            accountName: 'Acct One',
            nickname: 'Acct One',
            organizationId: 2,
            organizationName: 'Org',
            organizationType: 'enterprise',
          },
        ],
      });
      render(<ManagerAccount />);
      expect(screen.getByText('Choose an account')).toBeInTheDocument();
    });

    it('renders EmployeeList when session is present', () => {
      applyMock({
        session: { userId: 9, accountId: 16, accountName: 'client_uat' },
        employees: [],
      });
      render(<ManagerAccount />);
      expect(screen.getByText('Digital Employees')).toBeInTheDocument();
    });
  });
});
