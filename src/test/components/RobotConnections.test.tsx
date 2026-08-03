import { invoke } from '@tauri-apps/api/core';
import { render, screen } from '../helpers/render';
import { RobotConnections } from '@/components/RobotConnections';

const mockedInvoke = vi.mocked(invoke);

vi.mock('@/components/ManagerAccount', () => ({
  ManagerAccount: () => <div data-testid="manager-account" />,
}));

describe('RobotConnections', () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
  });

  it('renders the Manager login loop as the only connection source', () => {
    render(<RobotConnections />);

    expect(screen.getByTestId('manager-account')).toBeInTheDocument();
    expect(screen.queryByText(/Manual SMCP/i)).not.toBeInTheDocument();
    expect(screen.queryByRole('tab')).not.toBeInTheDocument();
  });

  it('does not load or mutate Manual SMCP targets', () => {
    render(<RobotConnections />);

    expect(mockedInvoke).not.toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(mockedInvoke).not.toHaveBeenCalledWith('save_manual_smcp_target', expect.anything());
    expect(mockedInvoke).not.toHaveBeenCalledWith('delete_manual_smcp_target', expect.anything());
  });
});
