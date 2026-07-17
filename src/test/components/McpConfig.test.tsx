import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { McpConfig } from '@/components/McpConfig';

const mockStore = {
  servers: [],
  loading: false,
  error: null,
  fetchServers: vi.fn(),
  addServer: vi.fn(),
  updateServer: vi.fn(),
  removeServer: vi.fn(),
  startServer: vi.fn(),
  stopServer: vi.fn(),
  startAll: vi.fn(),
  stopAll: vi.fn(),
  getServerConfig: vi.fn(),
  importConfig: vi.fn(),
  exportConfig: vi.fn(),
};

const { getInput, setValue } = vi.hoisted(() => ({
  getInput: vi.fn(),
  setValue: vi.fn(),
}));

vi.mock('@/stores/mcpStore', () => ({
  useMcpStore: vi.fn(() => mockStore),
}));

vi.mock('@/stores/inputStore', () => ({
  useInputStore: (selector: (state: unknown) => unknown) => selector({ getInput, setValue }),
}));

vi.mock('@/components/McpConfig/McpServerForm', () => ({
  McpServerForm: ({ onSubmit }: { onSubmit: (config: unknown) => Promise<void> }) => (
    <button
      onClick={() => onSubmit({
        type: 'Stdio',
        name: 'runtime-server',
        disabled: true,
        forbidden_tools: [],
        tool_meta: {},
        server_parameters: { command: 'echo', args: ['${input:api-key}'], env: {} },
      })}
    >
      Submit mocked server
    </button>
  ),
}));

import { useMcpStore } from '@/stores/mcpStore';
const mockUseMcpStore = vi.mocked(useMcpStore);

const mockServers = [
  {
    name: 'test-stdio',
    running: true,
    status_message: 'Running',
    disabled: false,
    managedBy: { type: 'user' },
  },
  {
    name: 'test-http',
    running: false,
    status_message: 'Stopped',
    disabled: false,
    managedBy: { type: 'user' },
  },
];

const mockPluginServers = [
  {
    name: 'plugin-tools',
    running: false,
    status_message: 'Stopped',
    disabled: false,
    managedBy: {
      type: 'plugin',
      marketplace: 'tf-market',
      plugin: 'desktop-tools',
      pluginId: 'plugin-1',
    },
  },
];

describe('McpConfig', () => {
  const instanceId = 'computer-a';

  beforeEach(() => {
    vi.clearAllMocks();
    getInput.mockResolvedValue({
      type: 'PromptString',
      id: 'api-key',
      label: 'API Key',
      password: true,
    });
    setValue.mockResolvedValue(undefined);
    mockUseMcpStore.mockReturnValue({ ...mockStore, servers: [] } as any);
  });

  it('calls fetchServers on mount', () => {
    render(<McpConfig instanceId={instanceId} />);
    expect(mockStore.fetchServers).toHaveBeenCalledWith(instanceId);
  });

  it('keeps configuration CRUD and import/export out of runtime controls', () => {
    render(<McpConfig instanceId={instanceId} />);
    expect(screen.getByText('MCP Servers')).toBeInTheDocument();
    expect(screen.getByText('Add Server')).toBeInTheDocument();
    expect(screen.getByText('Import Config')).toBeInTheDocument();
    expect(screen.getByText('Export Config')).toBeInTheDocument();
    expect(screen.queryByText('Start All')).not.toBeInTheDocument();
    expect(screen.queryByText('Stop All')).not.toBeInTheDocument();
  });

  it('shows only lifecycle actions in runtime mode', () => {
    render(<McpConfig instanceId={instanceId} mode="runtime" runtimeDisabled />);
    expect(screen.getByRole('button', { name: /Start All$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Stop All$/ })).toBeDisabled();
    expect(screen.queryByText('Add Server')).not.toBeInTheDocument();
    expect(screen.queryByText('Import Config')).not.toBeInTheDocument();
    expect(screen.queryByText('Export Config')).not.toBeInTheDocument();
  });

  it('renders error alert when error exists', () => {
    mockUseMcpStore.mockReturnValue({ ...mockStore, error: 'Something broke' } as any);
    render(<McpConfig instanceId={instanceId} />);
    expect(screen.getByText('Something broke')).toBeInTheDocument();
  });

  it('renders server list with servers', () => {
    mockUseMcpStore.mockReturnValue({ ...mockStore, servers: mockServers } as any);
    render(<McpConfig instanceId={instanceId} />);
    expect(screen.getByText('test-stdio')).toBeInTheDocument();
    expect(screen.getByText('test-http')).toBeInTheDocument();
    expect(screen.getAllByText('User').length).toBeGreaterThan(0);
  });

  it('shows plugin source and disables plugin-managed row actions', () => {
    mockUseMcpStore.mockReturnValue({ ...mockStore, servers: mockPluginServers } as any);

    render(<McpConfig instanceId={instanceId} />);

    expect(screen.getByText('plugin-tools')).toBeInTheDocument();
    expect(screen.getByText('desktop-tools@tf-market')).toBeInTheDocument();
    expect(screen.getByTitle('Edit')).toBeDisabled();
    expect(screen.getByTitle('Remove')).toBeDisabled();
  });

  it('disables plugin-managed lifecycle actions in runtime mode', () => {
    mockUseMcpStore.mockReturnValue({ ...mockStore, servers: mockPluginServers } as any);

    render(<McpConfig instanceId={instanceId} mode="runtime" />);

    expect(screen.getByTitle('Start')).toBeDisabled();
    expect(screen.queryByTitle('Edit')).not.toBeInTheDocument();
    expect(screen.queryByTitle('Remove')).not.toBeInTheDocument();
  });

  it('prompts for a missing input and retries the original MCP reload action', async () => {
    const missing = {
      code: 'missing_secret',
      input_id: 'api-key',
      env_hint: 'A2C_INPUT_API_KEY',
      message: 'Required secret input is unresolved',
    };
    mockStore.addServer
      .mockRejectedValueOnce(missing)
      .mockResolvedValueOnce(undefined);

    render(<McpConfig instanceId={instanceId} />);
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Submit mocked server' }));

    expect(await screen.findByText('Secret required to start')).toBeInTheDocument();
    const input = await screen.findByPlaceholderText('Enter value');
    fireEvent.change(input, { target: { value: 'top-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(mockStore.addServer).toHaveBeenCalledTimes(2));
    expect(setValue).toHaveBeenCalledWith(instanceId, 'api-key', 'top-secret');
    expect(mockStore.addServer.mock.calls[1]).toEqual(mockStore.addServer.mock.calls[0]);
    await waitFor(() => {
      expect(screen.queryByText('Secret required to start')).not.toBeInTheDocument();
    });
  }, 10000);
});
