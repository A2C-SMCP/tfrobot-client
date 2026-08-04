import { invoke } from '@tauri-apps/api/core';
import { render, screen } from '../helpers/render';
import { RobotConnections } from '@/components/RobotConnections';
import { useManagerStore } from '@/stores/managerStore';

const mockedInvoke = vi.mocked(invoke);

vi.mock('@/components/ManagerAccount/EmployeeList', () => ({
  EmployeeList: ({ showIdentityActions }: { showIdentityActions?: boolean }) => (
    <div data-testid="employee-list" data-identity-actions={String(showIdentityActions)} />
  ),
}));

describe('RobotConnections', () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    useManagerStore.setState(useManagerStore.getInitialState(), true);
  });

  it('guides signed-out users to the global account entry without owning login', () => {
    render(<RobotConnections />);

    expect(screen.getByText(/Sign in from the global Manager Account entry/)).toBeInTheDocument();
    expect(screen.queryByTestId('employee-list')).not.toBeInTheDocument();
    expect(screen.queryByRole('tab')).not.toBeInTheDocument();
  });

  it('shows current Context resources without page-owned logout controls', () => {
    useManagerStore.getState().applyContext({
      revision: 1,
      authState: 'authenticated',
      environment: 'staging',
      contextKey: {
        environment: 'staging',
        accountId: 'account-a',
        organizationId: 'organization-a',
      },
      user: { id: 'user-a', nickname: 'User A', email: '', phone: '' },
      account: { id: 'account-a', name: 'Account A', nickname: '', avatar: '', employeeNo: '' },
      organization: { id: 'organization-a', name: 'Organization A', organizationType: 'team' },
      permissions: [],
    });

    render(<RobotConnections />);

    expect(screen.getByTestId('employee-list')).toHaveAttribute('data-identity-actions', 'false');
  });

  it('does not load or mutate Manual SMCP targets', () => {
    render(<RobotConnections />);

    expect(mockedInvoke).not.toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(mockedInvoke).not.toHaveBeenCalledWith('save_manual_smcp_target', expect.anything());
    expect(mockedInvoke).not.toHaveBeenCalledWith('delete_manual_smcp_target', expect.anything());
  });
});
