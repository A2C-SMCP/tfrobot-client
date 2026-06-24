import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { RobotConnections } from '@/components/RobotConnections';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';

const mockedInvoke = vi.mocked(invoke);

vi.mock('@/components/ManagerAccount', () => ({
  ManagerAccount: () => <div data-testid="manager-account" />,
}));

describe('RobotConnections', () => {
  beforeEach(() => {
    useConnectionTargetStore.setState({
      manualTargets: [],
      loading: false,
      error: null,
    });
    mockedInvoke.mockReset();
  });

  it('manages connection resources without selecting or connecting a Computer', async () => {
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') {
        return [
          {
            id: 'manual-a',
            name: 'Manual A',
            url: 'https://smcp.example.com',
            namespace: '/smcp',
            office_id: 'office-a',
            computer_name: 'computer-a',
            headers: {},
          },
        ];
      }
      return null;
    });

    render(<RobotConnections />);

    fireEvent.click(screen.getByRole('tab', { name: /Manual SMCP/i }));

    expect(await screen.findByText('Manual A')).toBeInTheDocument();
    expect(screen.queryByText('Target Computer')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Connect/i })).not.toBeInTheDocument();
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('list_manual_smcp_targets');
    });
    expect(mockedInvoke).not.toHaveBeenCalledWith('list_computer_instances');
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_connection_target', expect.anything());
  }, 20000);

  it('does not show auto connection settings in the global Manual SMCP form', async () => {
    mockedInvoke.mockResolvedValue([]);

    render(<RobotConnections />);

    fireEvent.click(screen.getByRole('tab', { name: /Manual SMCP/i }));
    fireEvent.click(await screen.findByRole('button', { name: /Add Profile/i }));

    expect(screen.getAllByText('Add Profile').length).toBeGreaterThan(0);
    expect(screen.queryByText('Auto Connect')).not.toBeInTheDocument();
    expect(screen.queryByText('Auto Reconnect')).not.toBeInTheDocument();
  }, 20000);
});
