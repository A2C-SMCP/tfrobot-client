import { render, screen, fireEvent } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { Computer } from '@/components/Computer';
import { useComputerStore } from '@/stores/computerStore';

const mockInvoke = vi.mocked(invoke);

const mockComputerInstances = [
  {
    id: 'default',
    name: 'prod',
    is_default: true,
    running: true,
    connected: true,
    mcp_server_count: 5,
    robot_binding: null,
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
    id: 'default',
    name: 'Default Computer',
    is_default: true,
    mcp_server_count: 1,
  },
  {
    ...mockComputerInstances[0],
    id: 'computer-b',
    name: 'Second Computer',
    is_default: false,
    connected: false,
    mcp_server_count: 2,
    connection: null,
  },
];

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

describe('Computer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useComputerStore.getState().reset();
    mockInvoke.mockResolvedValue(mockComputerInstances);
  });

  it('renders the Computer list', () => {
    mockInvoke.mockReturnValueOnce(new Promise(() => {}));
    useComputerStore.setState({
      instances: [
        {
          id: 'default',
          name: 'prod',
          isDefault: true,
          status: 'running',
          connectionStatus: 'connected',
          connectionProfile: 'prod',
          mcpServerCount: 5,
        },
      ],
    });

    render(<Computer />);

    expect(screen.getByText('prod')).toBeInTheDocument();
    expect(screen.getByText('No robot bound')).toBeInTheDocument();
    expect(screen.getByText('Connection profile: prod')).toBeInTheDocument();
    expect(screen.getByText('5 MCP servers')).toBeInTheDocument();
  });

  it('renders an empty state when the Computer list is empty', async () => {
    mockInvoke.mockResolvedValueOnce([]);

    render(<Computer />);

    expect(await screen.findByText('No Computer instances')).toBeInTheDocument();
    expect(screen.queryByText('Open Details')).not.toBeInTheDocument();
  });

  it('opens a Computer detail view from the list', async () => {
    render(<Computer />);

    fireEvent.click(await screen.findByText('Open Details'));

    expect(screen.getByText('MCP Servers')).toBeInTheDocument();
    expect(screen.getByText('Input Variables')).toBeInTheDocument();
    expect(screen.getByText('Robot Connection')).toBeInTheDocument();
    expect(screen.getByText('Desktop Resources')).toBeInTheDocument();
    expect(screen.getByText('Debug Panel')).toBeInTheDocument();
    expect(screen.getByText('Logs')).toBeInTheDocument();
    expect(screen.getByText('Runtime')).toBeInTheDocument();
    expect(screen.getByTestId('mcp-config')).toHaveTextContent('default');
  });

  it('opens the selected second Computer with scoped detail tabs', async () => {
    mockInvoke.mockResolvedValueOnce(twoComputerInstances);

    render(<Computer />);

    fireEvent.click(await screen.findByText('Second Computer'));
    fireEvent.click(screen.getAllByText('Open Details')[1]);

    expect(screen.getByText('Second Computer')).toBeInTheDocument();
    expect(screen.getByTestId('mcp-config')).toHaveTextContent('computer-b');
    fireEvent.click(screen.getByText('Input Variables'));
    expect(screen.getByTestId('input-variables')).toHaveTextContent('computer-b');
  });

  it('can render the detail view directly', async () => {
    render(<Computer initialView="detail" initialTab="connection" />);

    expect(await screen.findByText('prod')).toBeInTheDocument();
    expect(screen.getByText('Back to Computers')).toBeInTheDocument();
    expect(screen.getByTestId('robot-connection-panel')).toHaveTextContent('default');
  });

  it('keeps detail tabs reachable from loaded Computer instances', async () => {
    render(<Computer initialView="detail" initialTab="debug" />);

    expect(await screen.findByText('prod')).toBeInTheDocument();
    expect(screen.getByText('Back to Computers')).toBeInTheDocument();
    expect(screen.getByTestId('debug-panel')).toHaveTextContent('default');
  });

  it('can return from detail to list', async () => {
    render(<Computer initialView="detail" />);

    fireEvent.click(await screen.findByText('Back to Computers'));

    expect(screen.getByText('Create Computer')).toBeInTheDocument();
    expect(screen.getByText('Open Details')).toBeInTheDocument();
  });
});
