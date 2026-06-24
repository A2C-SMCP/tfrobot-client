import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import { useComputerStore } from '@/stores/computerStore';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';
import { useManagerStore } from '@/stores/managerStore';

const mockedInvoke = vi.mocked(invoke);

vi.setConfig({ testTimeout: 60_000 });

describe('RobotConnectionPanel', () => {
  beforeEach(() => {
    useConnectionTargetStore.setState({
      manualTargets: [],
      loading: false,
      error: null,
    });
    useManagerStore.getState().reset();
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'Computer A',
          status: 'running',
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

  it('shows a Manager login guide without querying connection status', async () => {
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') return [];
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(
      await screen.findByText(/No Manager account is signed in/i),
    ).toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(mockedInvoke).not.toHaveBeenCalledWith('get_connection_status', expect.anything());
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_computer_connection_target', expect.anything());
  }, 20000);

  it('lists Manager Robots as read-only resources when signed in', async () => {
    useManagerStore.setState({
      session: { userId: 1, accountId: 23, accountName: 'acct' },
      employees: [
        {
          id: 11,
          name: 'Robot A',
          status: 'running',
          robotId: 'robot-a',
          namespace: 'org-a',
          clusterName: 'staging',
        },
      ],
      lastFetchAt: Date.now(),
    });
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') return [];
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(await screen.findByText('Robot A')).toBeInTheDocument();
    expect(screen.getByText('robot-a')).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_computer_connection_target', expect.anything());
    expect(mockedInvoke).not.toHaveBeenCalledWith('update_computer_connection_policy', expect.anything());
  }, 20000);

  it('shows Manual SMCP targets and opens target details', async () => {
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') {
        return [
          {
            id: 'target-a',
            name: 'Target A',
            url: 'https://smcp.example.com',
            namespace: '/smcp',
            office_id: 'office-a',
            computer_name: 'computer-a',
            headers: { 'x-env': 'dev' },
          },
        ];
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.click(screen.getByRole('tab', { name: /Manual SMCP/i }));

    expect(await screen.findByText('Target A')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Details/i }));

    await waitFor(() => {
      expect(screen.getAllByText('https://smcp.example.com').length).toBeGreaterThan(1);
      expect(screen.getByText('{"x-env":"dev"}')).toBeInTheDocument();
    });
  }, 20000);

  it('saves the selected target as Computer connection policy without connecting', async () => {
    mockedInvoke.mockImplementation(async (cmd) => {
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
          running: true,
          connected: false,
          mcp_server_count: 0,
          robot_binding: null,
          connection_policy: {
            target: { type: 'manual_smcp', id: 'target-a' },
            auto_connect: true,
          },
          connection: null,
        };
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

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
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_computer_connection_target', expect.anything());
  }, 20000);

  it('only shows running Manager Robots as selectable targets', async () => {
    useManagerStore.setState({
      session: { userId: 1, accountId: 23, accountName: 'acct' },
      employees: [
        {
          id: 24,
          name: 'Deleted Robot',
          status: 'deleted',
          templateDisplayName: 'UAT',
          templateType: 'tfrserver',
          namespace: 'org-a',
        },
        {
          id: 25,
          name: 'Running Robot',
          status: 'running',
          robotAccountId: 2525,
          templateDisplayName: 'UAT',
          templateType: 'tfrserver',
          namespace: 'org-a',
        },
      ],
      lastFetchAt: Date.now(),
    });
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') return [];
      if (cmd === 'update_computer_connection_policy') {
        return {
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: false,
          mcp_server_count: 0,
          robot_binding: null,
          connection_policy: {
            target: { type: 'manager_robot', id: '25', robotAccountId: 2525 },
            auto_connect: false,
          },
          connection: null,
        };
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    await waitFor(() => {
      expect(screen.queryByText('Deleted Robot (deleted / UAT / tfrserver / org-a)')).not.toBeInTheDocument();
    });
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: { type: 'manager_robot', id: '25', robotAccountId: 2525 },
          autoConnect: false,
        },
      });
    });
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_computer_connection_target', expect.anything());
  }, 20000);

  it('saves Manager Robot target with robotAccountId for Computer-level connect', async () => {
    useManagerStore.setState({
      session: { userId: 1, accountId: 23, accountName: 'acct' },
      employees: [
        {
          id: 25,
          name: 'Running Robot',
          status: 'running',
          robotAccountId: 2525,
          templateDisplayName: 'UAT',
          templateType: 'tfrserver',
          namespace: 'org-a',
        },
      ],
      lastFetchAt: Date.now(),
    });
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') return [];
      if (cmd === 'update_computer_connection_policy') {
        return {
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: false,
          mcp_server_count: 0,
          robot_binding: null,
          connection_policy: {
            target: { type: 'manager_robot', id: '25', robotAccountId: 2525 },
            auto_connect: false,
          },
          connection: null,
        };
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: { type: 'manager_robot', id: '25', robotAccountId: 2525 },
          autoConnect: false,
        },
      });
    });
  }, 20000);

  it('hydrates a saved Manager Robot target missing robotAccountId before saving', async () => {
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'Computer A',
          status: 'running',
          connectionStatus: 'disconnected',
          connectionPolicy: {
            target: { type: 'manager_robot', id: '25' },
            auto_connect: false,
          },
          mcpServerCount: 0,
        },
      ],
      loading: false,
      error: null,
      selectedInstanceId: 'computer-a',
    });
    useManagerStore.setState({
      session: { userId: 1, accountId: 23, accountName: 'acct' },
      employees: [
        {
          id: 25,
          name: 'Running Robot',
          status: 'running',
          robotAccountId: 2525,
          templateDisplayName: 'UAT',
          templateType: 'tfrserver',
          namespace: 'org-a',
        },
      ],
      lastFetchAt: Date.now(),
    });
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') return [];
      if (cmd === 'update_computer_connection_policy') {
        return {
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: false,
          mcp_server_count: 0,
          robot_binding: null,
          connection_policy: {
            target: { type: 'manager_robot', id: '25', robotAccountId: 2525 },
            auto_connect: false,
          },
          connection: null,
        };
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: { type: 'manager_robot', id: '25', robotAccountId: 2525 },
          autoConnect: false,
        },
      });
    });
  }, 20000);
});
