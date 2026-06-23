import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import { useComputerStore } from '@/stores/computerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';

vi.mock('@/components/ManagerAccount', () => ({
  ManagerAccount: ({ instanceId }: { instanceId?: string }) => (
    <div data-testid="manager-account">ManagerAccount:{instanceId}</div>
  ),
}));

const mockedInvoke = vi.mocked(invoke);

describe('RobotConnectionPanel', () => {
  beforeEach(() => {
    useConnectionStore.setState({
      statuses: {},
      loading: false,
      error: null,
    });
    useConnectionTargetStore.setState({
      manualTargets: [],
      loading: false,
      error: null,
    });
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'Computer A',
          status: 'stopped',
          connectionStatus: 'disconnected',
          connectionPolicy: { target: null, auto_connect: false },
          mcpServerCount: 0,
        },
      ],
      loading: false,
      error: null,
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockReset();
  });

  it('scopes Manager Robot connection controls to the current Computer', async () => {
    mockedInvoke.mockResolvedValueOnce({ connected: false });
    mockedInvoke.mockResolvedValueOnce([]);

    render(<RobotConnectionPanel instanceId="computer-a" />);

    expect(await screen.findByTestId('manager-account')).toHaveTextContent(
      'ManagerAccount:computer-a',
    );
    expect(screen.getByText('Manual SMCP')).toBeInTheDocument();
  }, 15000);

  it('saves auto connect as a Computer-scoped connection policy', async () => {
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'get_connection_status') return { connected: false };
      if (cmd === 'list_manual_smcp_targets') {
        return [
          {
            id: 'target-a',
            name: 'Target A',
            url: 'https://smcp.example.com',
            namespace: '/smcp',
            office_id: 'office-a',
            computer_name: 'computer-a',
            headers: {},
          },
        ];
      }
      if (cmd === 'update_computer_connection_policy') {
        return {
          id: 'computer-a',
          name: 'Computer A',
          running: false,
          connected: false,
          mcp_server_count: 0,
          robot_binding: null,
          connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: true },
          connection: null,
        };
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    fireEvent.click(await screen.findByText('Target A (office-a)'));
    fireEvent.click(screen.getByRole('switch'));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: { type: 'manual_smcp', id: 'target-a' },
          autoConnect: true,
        },
      });
    });
  }, 10000);
});
