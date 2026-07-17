import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import { useComputerStore } from '@/stores/computerStore';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';
import { useManagerStore } from '@/stores/managerStore';
import { runtimeSnapshot } from '../helpers/store';

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
          runtime: runtimeSnapshot(),
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

  it('auto-saves the selected Manual SMCP target without connecting', async () => {
    mockedInvoke.mockImplementation(async (cmd, args) => {
      if (cmd === 'list_manual_smcp_targets') {
        return [
          {
            id: 'target-a',
            name: 'Target A',
            url: 'https://smcp.example.com',
            namespace: '/smcp',
            office_id: 'office-a',
            headers: {},
          },
        ];
      }
      if (cmd === 'update_computer_connection_policy') {
        const request = (args as { request: { target: unknown; autoConnect: boolean } }).request;
        return {
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: false,
          mcp_server_count: 0,
          runtime: runtimeSnapshot(),
          robot_binding: null,
          connection_policy: {
            target: request.target,
            auto_connect: request.autoConnect,
          },
          connection: null,
        };
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    fireEvent.click(await screen.findByText('Target A (office-a)'));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: { type: 'manual_smcp', id: 'target-a' },
          autoConnect: false,
        },
      });
    });
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_computer_connection_target', expect.anything());
  }, 20000);

  it('rolls back the target selector when auto-save fails', async () => {
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'Computer A',
          status: 'running',
          connectionStatus: 'disconnected',
          connectionPolicy: {
            target: { type: 'manual_smcp', id: 'target-a' },
            auto_connect: false,
          },
          mcpServerCount: 0,
          runtime: runtimeSnapshot(),
        },
      ],
      loading: false,
      error: null,
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_manual_smcp_targets') {
        return [
          {
            id: 'target-a',
            name: 'Target A',
            url: 'https://smcp.example.com/a',
            namespace: '/smcp',
            office_id: 'office-a',
            headers: {},
          },
          {
            id: 'target-b',
            name: 'Target B',
            url: 'https://smcp.example.com/b',
            namespace: '/smcp',
            office_id: 'office-b',
            headers: {},
          },
        ];
      }
      if (cmd === 'update_computer_connection_policy') {
        throw new Error('save failed');
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    fireEvent.click(await screen.findByText('Target B (office-b)'));

    await waitFor(() => {
      expect(screen.getByText('Target A (office-a)')).toBeInTheDocument();
    });
  }, 20000);

  it('auto-saves auto-connect changes for the current target', async () => {
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'Computer A',
          status: 'running',
          connectionStatus: 'disconnected',
          connectionPolicy: {
            target: { type: 'manual_smcp', id: 'target-a' },
            auto_connect: false,
          },
          mcpServerCount: 0,
          runtime: runtimeSnapshot(),
        },
      ],
      loading: false,
      error: null,
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockImplementation(async (cmd, args) => {
      if (cmd === 'list_manual_smcp_targets') {
        return [
          {
            id: 'target-a',
            name: 'Target A',
            url: 'https://smcp.example.com',
            namespace: '/smcp',
            office_id: 'office-a',
            headers: {},
          },
        ];
      }
      if (cmd === 'update_computer_connection_policy') {
        const request = (args as { request: { target: unknown; autoConnect: boolean } }).request;
        return {
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: false,
          mcp_server_count: 0,
          runtime: runtimeSnapshot(),
          robot_binding: null,
          connection_policy: {
            target: request.target,
            auto_connect: request.autoConnect,
          },
          connection: null,
        };
      }
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole('switch'));

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
          runtime: runtimeSnapshot(),
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
          runtime: runtimeSnapshot(),
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

  it('auto-hydrates a saved Manager Robot target missing robotAccountId', async () => {
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
          runtime: runtimeSnapshot(),
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
          runtime: runtimeSnapshot(),
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
