import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import { useComputerStore } from '@/stores/computerStore';
import { useManagerStore } from '@/stores/managerStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

function setComputer(
  target: { type: 'manager_robot' | 'manual_smcp'; id: string; robotAccountId?: string } | null = null,
) {
  useComputerStore.setState({
    instances: [
      {
        id: 'computer-a',
        name: 'Computer A',
        status: 'running',
        connectionStatus: 'disconnected',
        connectionPolicy: { target, auto_connect: false },
        mcpServerCount: 0,
        runtime: runtimeSnapshot(),
      },
    ],
    loading: false,
    error: null,
    selectedInstanceId: 'computer-a',
  });
}

function setManagerRobots() {
  useManagerStore.setState({
    session: { userId: 1, accountId: 'org-1:account-23', accountName: 'acct' },
    employees: [
      {
        id: 24,
        name: 'Deleted Robot',
        status: 'deleted',
        robotAccountId: '2424',
      },
      {
        id: 25,
        name: 'Running Robot',
        status: 'running',
        robotAccountId: '2525',
        robotId: 'robot-25',
        templateDisplayName: 'UAT',
        templateType: 'tfrserver',
        namespace: 'org-a',
      },
    ],
    lastFetchAt: Date.now(),
  });
}

function managerPolicyResponse() {
  return {
    id: 'computer-a',
    name: 'Computer A',
    running: true,
    connected: false,
    mcp_server_count: 0,
    runtime: runtimeSnapshot(),
    robot_binding: null,
    connection_policy: {
      target: { type: 'manager_robot', id: '25', robotAccountId: '2525' },
      auto_connect: false,
    },
    connection: null,
  };
}

describe('RobotConnectionPanel', () => {
  beforeEach(() => {
    useManagerStore.getState().reset();
    setComputer();
    mockedInvoke.mockReset();
  });

  it('shows a Manager login guide and never loads Manual SMCP targets', async () => {
    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(await screen.findByText(/No Manager account is signed in/i)).toBeInTheDocument();
    expect(screen.queryByText(/Manual SMCP/i)).not.toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(mockedInvoke).not.toHaveBeenCalledWith('get_connection_status', expect.anything());
  });

  it('hides a persisted legacy Manual target without clearing it', () => {
    setComputer({ type: 'manual_smcp', id: 'legacy-target' });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(screen.queryByText('legacy-target')).not.toBeInTheDocument();
    expect(screen.getByRole('switch', { name: 'Auto Connect' })).toBeDisabled();
    expect(mockedInvoke).not.toHaveBeenCalledWith('update_computer_connection_policy', expect.anything());
  });

  it('lists Manager Robots as read-only resources when signed in', async () => {
    setManagerRobots();

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(await screen.findByText('Running Robot')).toBeInTheDocument();
    expect(screen.getByText('robot-25')).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_computer_connection_target', expect.anything());
  });

  it('only exposes a running Manager Robot as a selectable policy target', async () => {
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') return managerPolicyResponse();
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    expect(screen.queryByText('Deleted Robot (deleted)')).not.toBeInTheDocument();
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: { type: 'manager_robot', id: '25', robotAccountId: '2525' },
          autoConnect: false,
        },
      });
    });
  });

  it('rolls back a Manager Robot selection when policy persistence fails', async () => {
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') throw new Error('save failed');
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', expect.anything());
      expect(screen.getByText('Select a Manager Robot')).toBeInTheDocument();
    });
    expect(screen.getByRole('switch', { name: 'Auto Connect' })).toBeDisabled();
  });

  it('hydrates a saved Manager Robot policy with its opaque connection account metadata', async () => {
    setComputer({ type: 'manager_robot', id: '25' });
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') return managerPolicyResponse();
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: { type: 'manager_robot', id: '25', robotAccountId: '2525' },
          autoConnect: false,
        },
      });
    });
  });
});
