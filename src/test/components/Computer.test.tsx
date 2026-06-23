import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { Computer } from '@/components/Computer';
import { useComputerStore } from '@/stores/computerStore';

const mockInvoke = vi.mocked(invoke);

const mockComputerInstances = [
  {
    id: 'computer-a',
    name: 'prod',
    description: 'Production computer',
    running: true,
    connected: true,
    mcp_server_count: 5,
    robot_binding: {
      employee_id: 42,
      robot_id: 'robot-a',
      robot_account_id: 4200,
      namespace: 'test',
      robot_name: 'Robot A',
    },
    connection_policy: { target: { type: 'manager_robot', id: '42' }, auto_connect: false },
    connection: {
      url: 'https://smcp.example.com',
      office_id: 'office-1',
      computer_name: 'prod',
      connected_at: '2026-01-01T00:00:00Z',
      profile_name: 'prod',
    },
  },
];

const twoComputerInstances = [
  {
    ...mockComputerInstances[0],
    id: 'computer-a',
    name: 'Computer A',
    mcp_server_count: 1,
  },
  {
    ...mockComputerInstances[0],
    id: 'computer-b',
    name: 'Second Computer',
    connected: false,
    mcp_server_count: 2,
    connection: null,
  },
];

function last<T>(items: T[]): T {
  return items[items.length - 1];
}

vi.mock('@/components/McpConfig', () => ({
  McpConfig: ({ instanceId }: { instanceId: string }) => <div data-testid="mcp-config">McpConfig:{instanceId}</div>,
}));
vi.mock('@/components/InputVariables', () => ({
  InputVariables: ({ instanceId }: { instanceId: string }) => <div data-testid="input-variables">InputVariables:{instanceId}</div>,
}));
vi.mock('@/components/RobotConnectionPanel', () => ({
  RobotConnectionPanel: ({ instanceId }: { instanceId: string }) => (
    <div data-testid="robot-connection-panel">RobotConnectionPanel:{instanceId}</div>
  ),
}));
vi.mock('@/components/DesktopResources', () => ({
  DesktopResources: ({ instanceId }: { instanceId: string }) => <div data-testid="desktop-resources">DesktopResources:{instanceId}</div>,
}));
vi.mock('@/components/DebugPanel', () => ({
  DebugPanel: ({ instanceId }: { instanceId: string }) => <div data-testid="debug-panel">DebugPanel:{instanceId}</div>,
}));
vi.mock('@/components/LogViewer', () => ({
  LogViewer: ({ instanceId }: { instanceId?: string }) => <div data-testid="log-viewer">LogViewer:{instanceId}</div>,
}));
vi.mock('@/components/Settings/RuntimeSettings', () => ({
  RuntimeSettings: () => <div data-testid="runtime-settings">RuntimeSettings</div>,
}));
vi.mock('@/components/Computer/ComputerOverview', () => ({
  ComputerOverview: ({ instanceId }: { instanceId: string }) => <div data-testid="computer-overview">ComputerOverview:{instanceId}</div>,
}));

const { mockFetchManualTargets } = vi.hoisted(() => ({
  mockFetchManualTargets: vi.fn(),
}));

vi.mock('@/stores/connectionTargetStore', () => ({
  useConnectionTargetStore: vi.fn(() => ({
    manualTargets: [
      {
        id: 'target-a',
        name: 'Target A',
        office_id: 'office-a',
      },
    ],
    fetchManualTargets: mockFetchManualTargets,
  })),
}));

describe('Computer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockFetchManualTargets.mockReset();
    useComputerStore.getState().reset();
    mockInvoke.mockResolvedValue(mockComputerInstances);
  });

  it('renders the Computer list', () => {
    mockInvoke.mockReturnValueOnce(new Promise(() => {}));
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'prod',
          description: 'Production computer',
          status: 'running',
          connectionStatus: 'connected',
          connectionProfile: 'prod',
          robotName: 'Robot A',
          connectionPolicy: { target: { type: 'manager_robot', id: '42' }, auto_connect: false },
          mcpServerCount: 5,
        },
      ],
    });

    render(<Computer />);

    expect(screen.getByText('prod')).toBeInTheDocument();
    expect(screen.getByText('Production computer')).toBeInTheDocument();
    expect(screen.getByText('Bound robot: Robot A')).toBeInTheDocument();
    expect(screen.getByText('Connection profile: prod')).toBeInTheDocument();
    expect(screen.getByText('5 MCP servers')).toBeInTheDocument();
  });

  it('renders an empty state when the Computer list is empty', async () => {
    mockInvoke.mockResolvedValueOnce([]);

    render(<Computer />);

    expect(await screen.findByText('No Computer instances')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open Details' })).not.toBeInTheDocument();
  });

  it('opens a Computer detail view from the list', async () => {
    render(<Computer />);

    fireEvent.click(await screen.findByRole('button', { name: 'prod' }));

    expect(screen.getByText('Overview')).toBeInTheDocument();
    expect(screen.getByText('MCP Servers')).toBeInTheDocument();
    expect(screen.getByText('Input Variables')).toBeInTheDocument();
    expect(screen.getByText('Robot Connection')).toBeInTheDocument();
    expect(screen.getByText('Desktop Resources')).toBeInTheDocument();
    expect(screen.getByText('Debug Panel')).toBeInTheDocument();
    expect(screen.getByText('Logs')).toBeInTheDocument();
    expect(screen.getByText('Runtime')).toBeInTheDocument();
    expect(screen.getByTestId('computer-overview')).toHaveTextContent('computer-a');
  }, 10000);

  it('opens the selected second Computer with scoped detail tabs', async () => {
    mockInvoke.mockResolvedValueOnce(twoComputerInstances);

    render(<Computer />);

    fireEvent.click(await screen.findByRole('button', { name: 'Second Computer' }));

    expect(screen.getByText('Second Computer')).toBeInTheDocument();
    expect(screen.getByTestId('computer-overview')).toHaveTextContent('computer-b');
    fireEvent.click(screen.getByText('MCP Servers'));
    expect(screen.getByTestId('mcp-config')).toHaveTextContent('computer-b');
    fireEvent.click(screen.getByText('Input Variables'));
    expect(screen.getByTestId('input-variables')).toHaveTextContent('computer-b');
  }, 10000);

  it('can render the detail view directly', async () => {
    render(<Computer initialView="detail" initialTab="connection" />);

    expect(await screen.findByText('prod')).toBeInTheDocument();
    expect(screen.getByText('Back to Computers')).toBeInTheDocument();
    expect(screen.getByTestId('robot-connection-panel')).toHaveTextContent('computer-a');
  });

  it('keeps detail tabs reachable from loaded Computer instances', async () => {
    render(<Computer initialView="detail" initialTab="debug" />);

    expect(await screen.findByText('prod')).toBeInTheDocument();
    expect(screen.getByText('Back to Computers')).toBeInTheDocument();
    expect(screen.getByTestId('debug-panel')).toHaveTextContent('computer-a');
  });

  it('can return from detail to list', async () => {
    render(<Computer initialView="detail" />);

    fireEvent.click(await screen.findByText('Back to Computers'));

    expect(screen.getByText('Create Computer')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'prod' })).toBeInTheDocument();
  }, 15000);

  it('disables list connect when the Computer is not running', async () => {
    const stoppedInstance = {
      id: 'computer-a',
      name: 'prod',
      running: false,
      connected: false,
      mcp_server_count: 0,
      robot_binding: null,
      connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
      connection: null,
    };
    mockInvoke.mockResolvedValueOnce([stoppedInstance]);

    render(<Computer />);

    expect(await screen.findByRole('button', { name: 'Connect' })).toBeDisabled();
  }, 15000);

  it('connects the selected target from the list action', async () => {
    const disconnectedInstance = {
      ...mockComputerInstances[0],
      connected: false,
      connection: null,
      connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    };
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return [disconnectedInstance];
      if (cmd === 'connect_computer_connection_target') return null;
      return null;
    });
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'prod',
          status: 'running',
          connectionStatus: 'disconnected',
          connectionPolicy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
          mcpServerCount: 0,
        },
      ],
    });

    render(<Computer />);

    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('connect_computer_connection_target', {
        id: 'computer-a',
      });
    });
  }, 10000);

  it('creates a Computer from the list page', async () => {
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return mockComputerInstances;
      if (cmd === 'create_computer_instance') {
        return {
          id: 'computer-new',
          name: 'New Computer',
          description: 'New description',
          running: false,
          connected: false,
          mcp_server_count: 0,
          robot_binding: null,
          connection: null,
        };
      }
      return null;
    });

    render(<Computer />);

    fireEvent.click(await screen.findByText('Create Computer'));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'New Computer' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'New description' } });
    fireEvent.click(last(screen.getAllByText('OK')));

    expect(await screen.findByText('New Computer')).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith('create_computer_instance', {
      request: { name: 'New Computer', description: 'New description' },
    });
  });

  it('edits, duplicates, starts, stops, and deletes from the list actions', async () => {
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return mockComputerInstances;
      if (cmd === 'rename_computer_instance') return { ...mockComputerInstances[0], name: 'Updated Computer', description: 'Updated description' };
      if (cmd === 'duplicate_computer_instance') return { ...mockComputerInstances[0], id: 'computer-copy', name: 'prod Copy', running: false };
      if (cmd === 'stop_computer_instance') return { ...mockComputerInstances[0], running: false };
      if (cmd === 'start_computer_instance') return { ...mockComputerInstances[0], running: true };
      if (cmd === 'delete_computer_instance') return null;
      return null;
    });

    render(<Computer />);

    fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0]);
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Updated Computer' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Updated description' } });
    fireEvent.click(last(screen.getAllByText('OK')));
    expect(await screen.findByText('Updated Computer')).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole('button', { name: 'Duplicate' })[0]);
    await waitFor(() => expect(mockFetchManualTargets).toHaveBeenCalled());
    expect(screen.getByLabelText('Copy Robot binding')).toBeChecked();
    fireEvent.mouseDown(screen.getByLabelText('Connection target'));
    fireEvent.click(await screen.findByText('Target A (office-a)'));
    fireEvent.click(last(screen.getAllByText('OK')));
    expect(await screen.findByText('prod Copy')).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith('duplicate_computer_instance', {
      request: {
        sourceId: 'computer-a',
        name: 'Updated Computer Copy',
        description: 'Updated description',
        copyRobotBinding: true,
        connectionTargetId: 'target-a',
      },
    });

    fireEvent.click(screen.getAllByRole('button', { name: 'Stop' })[0]);
    expect(await screen.findByText('Stopped')).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole('button', { name: 'Start' })[0]);
    expect(await screen.findByText('Running')).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole('button', { name: 'Delete' })[0]);
    fireEvent.click(last(await screen.findAllByText('OK')));
    expect(mockInvoke).toHaveBeenCalledWith('delete_computer_instance', { id: 'computer-a' });
  }, 80000);
});
