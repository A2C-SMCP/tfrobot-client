import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { McpConfig } from '@/components/McpConfig';
import { save } from '@tauri-apps/plugin-dialog';

const mockSdkStore = {
  snapshot: null,
  validation: null,
  loading: false,
  validating: false,
  error: null,
  fetchConfig: vi.fn(),
  validateConfig: vi.fn(),
  upsertServer: vi.fn(),
  removeServer: vi.fn(),
  importConfig: vi.fn(),
  exportConfig: vi.fn(),
};

const mockMcpStore = vi.hoisted(() => ({
  servers: [] as Array<{
    bundleId: string;
    name: string;
    activation_state: 'stopped' | 'started';
    connection_state: 'disconnected' | 'connecting' | 'connected' | 'authorization_required' | 'error';
    running: boolean;
    status_message: string;
    disabled: boolean;
    managedBy:
      | { type: 'user' }
      | { type: 'plugin'; marketplace: string; plugin: string; pluginId?: string | null };
  }>,
  activeInstanceId: 'computer-a' as string | null,
  serversReady: true,
  loading: false,
  error: null as string | null,
  fetchServers: vi.fn(),
}));

vi.mock('@/stores/sdkConfigStore', () => ({
  useSdkConfigStore: vi.fn(() => mockSdkStore),
}));

vi.mock('@/stores/mcpStore', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/stores/mcpStore')>();
  return {
    ...actual,
    useMcpStore: (selector: (state: typeof mockMcpStore) => unknown) => selector(mockMcpStore),
  };
});

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

import { useSdkConfigStore } from '@/stores/sdkConfigStore';
const mockUseSdkConfigStore = vi.mocked(useSdkConfigStore);

const configServers = [
  {
    bundleId: 'test-stdio',
    name: 'test-stdio',
    origin: 'local',
    writable: true,
    trustedOrigin: false,
    bundled: false,
    config: {
      type: 'Stdio',
      name: 'test-stdio',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { command: 'node', args: [], env: {} },
    },
  },
  {
    bundleId: 'plugin-tools',
    name: 'plugin-tools',
    origin: 'project',
    writable: true,
    trustedOrigin: true,
    bundled: true,
    config: {
      type: 'Stdio',
      name: 'plugin-tools',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { command: 'plugin', args: [], env: {} },
    },
  },
];

describe('McpConfig', () => {
  const instanceId = 'computer-a';

  beforeEach(() => {
    vi.clearAllMocks();
    mockMcpStore.servers = [];
    mockMcpStore.activeInstanceId = instanceId;
    mockMcpStore.serversReady = true;
    mockMcpStore.loading = false;
    mockMcpStore.error = null;
    mockMcpStore.fetchServers.mockResolvedValue(undefined);
    mockSdkStore.upsertServer.mockResolvedValue(undefined);
    mockUseSdkConfigStore.mockReturnValue({ ...mockSdkStore } as any);
  });

  it('loads the SDK config snapshot and validation on mount', () => {
    render(<McpConfig instanceId={instanceId} />);
    expect(mockSdkStore.fetchConfig).toHaveBeenCalledWith(instanceId);
    expect(mockMcpStore.fetchServers).toHaveBeenCalledWith(instanceId);
  });

  it('keeps configuration CRUD, import/export, and schema validation out of runtime controls', () => {
    render(<McpConfig instanceId={instanceId} />);
    expect(screen.getByText('MCP Configuration')).toBeInTheDocument();
    expect(screen.getByText('Add Server')).toBeInTheDocument();
    expect(screen.getByText('Import Config')).toBeInTheDocument();
    expect(screen.getByText('Export Config')).toBeInTheDocument();
    expect(screen.getByText('Validate Schema')).toBeInTheDocument();
    expect(screen.getByText(/does not check commands, paths, secrets/)).toBeInTheDocument();
    expect(screen.queryByText('Start All')).not.toBeInTheDocument();
    expect(screen.queryByText('Stop All')).not.toBeInTheDocument();
  });

  it('requires explicit acknowledgement before exporting plaintext constants', async () => {
    vi.mocked(save).mockResolvedValue('/tmp/mcp-config.json');
    mockSdkStore.exportConfig.mockResolvedValue(undefined);
    render(<McpConfig instanceId={instanceId} />);

    fireEvent.click(screen.getByRole('button', { name: 'Export Config' }));

    expect((await screen.findAllByText('Export configuration with plaintext constants?')).length)
      .toBeGreaterThan(0);
    expect(save).not.toHaveBeenCalled();
    expect(mockSdkStore.exportConfig).not.toHaveBeenCalled();

    const confirmButtons = screen.getAllByRole('button', {
      name: 'Export plaintext configuration',
    });
    fireEvent.click(confirmButtons[confirmButtons.length - 1]);

    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(mockSdkStore.exportConfig).toHaveBeenCalledWith(
      instanceId,
      '/tmp/mcp-config.json',
    );
  }, 15_000);

  it('renders a safe error alert without exposing the stored technical value', () => {
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      error: 'spawn /secret/path failed with token=private',
    } as any);
    render(<McpConfig instanceId={instanceId} />);
    expect(screen.getByText(
      'The MCP operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/path|token=private/)).not.toBeInTheDocument();
  });

  it('renders SDK config revision, provenance, and schema result without runtime status', () => {
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:config',
        mcp: { servers: configServers },
        provenance: {},
      },
      validation: { valid: true, errors: [] },
    } as any);
    render(<McpConfig instanceId={instanceId} />);
    expect(screen.getAllByText('test-stdio')).toHaveLength(2);
    expect(screen.getByText('sha256:config')).toBeInTheDocument();
    expect(screen.getByText('local')).toBeInTheDocument();
    expect(screen.getByText('The SDK configuration schema is valid.')).toBeInTheDocument();
    expect(screen.queryByText('Running')).not.toBeInTheDocument();
    expect(screen.queryByText('Stopped')).not.toBeInTheDocument();
  });

  it('toggles a writable user MCP declaration from the config list', async () => {
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:config',
        mcp: { servers: [configServers[0]] },
        provenance: {},
      },
    } as any);
    render(<McpConfig instanceId={instanceId} />);

    const enabledSwitch = screen.getByRole('switch', {
      name: 'Toggle server test-stdio enabled state',
    });
    expect(enabledSwitch).toBeChecked();
    fireEvent.click(enabledSwitch);

    await waitFor(() => expect(mockSdkStore.upsertServer).toHaveBeenCalledWith(
      instanceId,
      expect.objectContaining({ name: 'test-stdio', disabled: true }),
    ));
  });

  it('fails closed while authoritative MCP ownership is loading', () => {
    mockMcpStore.serversReady = false;
    mockMcpStore.loading = true;
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:ownership-loading',
        mcp: { servers: [configServers[0]] },
        provenance: {},
      },
    } as any);

    render(<McpConfig instanceId={instanceId} />);

    expect(screen.getByText('Checking MCP declaration ownership')).toBeInTheDocument();
    expect(screen.getByRole('switch', {
      name: 'Toggle server test-stdio enabled state',
    })).toBeDisabled();
    expect(screen.getByTitle('Edit')).toBeDisabled();
    expect(screen.getByTitle('Remove')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Import Config' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Add Server' })).toBeDisabled();
  });

  it('fails closed when authoritative MCP ownership cannot be loaded', () => {
    mockMcpStore.serversReady = false;
    mockMcpStore.error = 'ownership lookup failed';
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:ownership-error',
        mcp: { servers: [configServers[0]] },
        provenance: {},
      },
    } as any);

    render(<McpConfig instanceId={instanceId} />);

    expect(screen.getByText('MCP declaration ownership is unavailable')).toBeInTheDocument();
    expect(screen.queryByText('ownership lookup failed')).not.toBeInTheDocument();
    expect(screen.getByRole('switch', {
      name: 'Toggle server test-stdio enabled state',
    })).toBeDisabled();
    expect(screen.getByTitle('Edit')).toBeDisabled();
    expect(screen.getByTitle('Remove')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Import Config' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Add Server' })).toBeDisabled();
  });

  it('refreshes both saved configuration and authoritative ownership', () => {
    render(<McpConfig instanceId={instanceId} />);

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));

    expect(mockSdkStore.fetchConfig).toHaveBeenCalledTimes(2);
    expect(mockMcpStore.fetchServers).toHaveBeenCalledTimes(2);
    expect(mockMcpStore.fetchServers).toHaveBeenLastCalledWith(instanceId);
  });

  it('keeps a missing definition in the MCP editing flow when enabling a declaration', async () => {
    mockSdkStore.upsertServer.mockRejectedValueOnce({
      code: 'missing_input_definition',
      input_id: 'api-key',
      message: 'Required input definition is missing',
      requesting_mcp: { bundle_id: 'test-stdio', name: 'test-stdio' },
    });
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:config',
        mcp: { servers: [configServers[0]] },
        provenance: {},
      },
    } as any);
    render(<McpConfig instanceId={instanceId} />);

    fireEvent.click(screen.getByRole('switch', {
      name: 'Toggle server test-stdio enabled state',
    }));

    expect(await screen.findByText(/test-stdio.*api-key.*definition is missing/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Configure api-key' })).not.toBeInTheDocument();
    expect(mockSdkStore.upsertServer).toHaveBeenCalledTimes(1);
  }, 10000);

  it('keys same-name server rows by BundleId', () => {
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:same-name',
        mcp: {
          servers: [
            { ...configServers[0], bundleId: 'server-a', name: 'shared-name' },
            { ...configServers[0], bundleId: 'server-b', name: 'shared-name' },
          ],
        },
        provenance: {},
      },
    } as any);

    const { container } = render(<McpConfig instanceId={instanceId} />);

    expect(container.querySelector('tr[data-row-key="server-a"]')).toBeInTheDocument();
    expect(container.querySelector('tr[data-row-key="server-b"]')).toBeInTheDocument();
  });

  it('shows validation errors but keeps bundled-name config declarations editable', () => {
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:config',
        mcp: { servers: configServers },
        provenance: {},
      },
      validation: {
        valid: false,
        errors: [{
          scope: 'project',
          source_path: 'mcp.json',
          field: 'servers.bad',
          reason: 'invalid transport',
        }],
      },
    } as any);

    render(<McpConfig instanceId={instanceId} />);

    expect(screen.getAllByText('plugin-tools')).toHaveLength(2);
    expect(screen.getByText('mcp.json:servers.bad: invalid transport')).toBeInTheDocument();
    expect(screen.getAllByTitle('Edit')[1]).toBeEnabled();
    expect(screen.getAllByTitle('Remove')[1]).toBeEnabled();
  });

  it('disables config mutations for declarations from read-only SDK origins', async () => {
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:policy',
        mcp: {
          servers: [{
            ...configServers[0],
            name: 'policy-server',
            origin: 'policy',
            writable: false,
            config: { ...configServers[0].config, name: 'policy-server' },
          }],
        },
        provenance: {},
      },
    } as any);

    render(<McpConfig instanceId={instanceId} />);

    const editButton = screen.getByTitle('Edit');
    expect(editButton).toBeDisabled();
    expect(screen.getByTitle('Remove')).toBeDisabled();
    expect(screen.getByRole('switch', {
      name: 'Toggle server policy-server enabled state',
    })).toBeDisabled();
    fireEvent.mouseOver(editButton.parentElement!);
    expect(await screen.findByText(/read-only policy scope/)).toBeInTheDocument();
  });

  it('shows Plugin ownership and opens the exact owning Plugin', () => {
    const onOpenPlugin = vi.fn();
    mockMcpStore.servers = [{
      bundleId: 'plugin-tools',
      name: 'plugin-tools',
      activation_state: 'stopped',
      connection_state: 'disconnected',
      running: false,
      status_message: 'Stopped',
      disabled: false,
      managedBy: {
        type: 'plugin',
        marketplace: 'acme',
        plugin: 'audit',
        pluginId: 'plugin-2',
      },
    }];
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:plugin',
        mcp: {
          servers: [{
            ...configServers[1],
            origin: 'local',
            writable: true,
          }],
        },
        provenance: {},
      },
    } as any);

    render(<McpConfig instanceId={instanceId} onOpenPlugin={onOpenPlugin} />);

    expect(screen.getAllByText(
      'Managed by plugin audit from acme. Use Marketplace to manage its lifecycle.',
    ).length).toBeGreaterThan(0);
    expect(screen.getByText(
      'This declaration comes from the read-only plugin scope and cannot be edited here.',
    )).toBeInTheDocument();
    expect(screen.getByRole('switch', {
      name: 'Toggle server plugin-tools enabled state',
    })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Manage audit' }));
    expect(onOpenPlugin).toHaveBeenCalledWith({
      type: 'plugin',
      marketplace: 'acme',
      plugin: 'audit',
      pluginId: 'plugin-2',
    });
  });

  it('uses the authoritative MCP owner after a shared Bundle changes Plugin ownership', () => {
    const onOpenPlugin = vi.fn();
    mockMcpStore.servers = [{
      bundleId: 'plugin-tools',
      name: 'plugin-tools',
      activation_state: 'stopped',
      connection_state: 'disconnected',
      running: false,
      status_message: 'Stopped',
      disabled: false,
      managedBy: {
        type: 'plugin',
        marketplace: 'acme',
        plugin: 'replacement',
        pluginId: 'plugin-3',
      },
    }];
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:plugin-handoff',
        mcp: {
          servers: [{
            ...configServers[1],
            origin: 'local',
            writable: true,
          }],
        },
        provenance: {},
      },
    } as any);

    render(<McpConfig instanceId={instanceId} onOpenPlugin={onOpenPlugin} />);

    fireEvent.click(screen.getByRole('button', { name: 'Manage replacement' }));
    expect(onOpenPlugin).toHaveBeenCalledWith({
      type: 'plugin',
      marketplace: 'acme',
      plugin: 'replacement',
      pluginId: 'plugin-3',
    });
  });

  it('restores writable config actions after authoritative ownership returns to the user', () => {
    mockMcpStore.servers = [{
      bundleId: 'plugin-tools',
      name: 'plugin-tools',
      activation_state: 'stopped',
      connection_state: 'disconnected',
      running: false,
      status_message: 'Stopped',
      disabled: false,
      managedBy: { type: 'user' },
    }];
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:user-handoff',
        mcp: {
          servers: [{
            ...configServers[1],
            origin: 'local',
            writable: true,
          }],
        },
        provenance: {},
      },
    } as any);

    render(<McpConfig instanceId={instanceId} />);

    expect(screen.getByRole('switch', {
      name: 'Toggle server plugin-tools enabled state',
    })).toBeEnabled();
    expect(screen.getByTitle('Edit')).toBeEnabled();
    expect(screen.queryByRole('button', { name: /Manage/ })).not.toBeInTheDocument();
  });

  it('persists a declaration through config CRUD when its inputs are resolved', async () => {
    render(<McpConfig instanceId={instanceId} />);
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Submit mocked server' }));

    await waitFor(() => expect(mockSdkStore.upsertServer).toHaveBeenCalledOnce());
    expect(mockSdkStore.upsertServer).toHaveBeenCalledWith(instanceId, expect.objectContaining({
      name: 'runtime-server',
    }));
    expect(screen.queryByText('Secret required to start')).not.toBeInTheDocument();
  }, 10000);

  it('keeps the MCP form open for a missing definition and succeeds after the corrected resubmit', async () => {
    mockSdkStore.upsertServer
      .mockRejectedValueOnce({
        code: 'missing_input_definition',
        input_id: 'api-key',
        message: 'Required input definition is missing',
        requesting_mcp: { bundle_id: 'runtime-server', name: 'runtime-server' },
      })
      .mockResolvedValueOnce(undefined);
    render(<McpConfig instanceId={instanceId} />);
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Submit mocked server' }));

    expect(await screen.findByText(/runtime-server.*api-key.*definition is missing/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Configure api-key' })).not.toBeInTheDocument();
    const resubmit = screen.getByRole('button', { name: 'Submit mocked server' });
    expect(resubmit).toBeInTheDocument();

    fireEvent.click(resubmit);
    await waitFor(() => expect(mockSdkStore.upsertServer).toHaveBeenCalledTimes(2));
    expect(await screen.findByText('Server saved; restart Runtime to apply it')).toBeInTheDocument();
  }, 10000);

  it('does not synthesize a local retry when the backend reports a legacy missing input', async () => {
    mockSdkStore.upsertServer.mockRejectedValueOnce({
      code: 'missing_secret',
      input_id: 'api-key',
      env_hint: 'A2C_SMCP_api_key',
      message: 'Required secret input is unresolved',
    });
    render(<McpConfig instanceId={instanceId} />);
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Submit mocked server' }));

    expect(await screen.findByText(
      'The MCP operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Configure api-key' })).not.toBeInTheDocument();
    expect(mockSdkStore.upsertServer).toHaveBeenCalledTimes(1);
  });

  it('does not expose technical errors when adding a server fails at runtime', async () => {
    mockSdkStore.upsertServer.mockRejectedValueOnce(
      new Error('spawn /secret/add failed with token=private'),
    );
    render(<McpConfig instanceId={instanceId} />);

    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Submit mocked server' }));

    expect(await screen.findByText(
      'The MCP operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/add|token=private/)).not.toBeInTheDocument();
  });

  it('does not expose technical errors when editing a server fails at runtime', async () => {
    mockSdkStore.upsertServer.mockRejectedValueOnce(
      new Error('unmount /secret/edit failed with token=private'),
    );
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:config',
        mcp: { servers: [configServers[0]] },
        provenance: {},
      },
    } as any);
    render(<McpConfig instanceId={instanceId} />);

    fireEvent.click(screen.getByTitle('Edit'));
    fireEvent.click(await screen.findByRole('button', { name: 'Submit mocked server' }));

    expect(await screen.findByText(
      'The MCP operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/edit|token=private/)).not.toBeInTheDocument();
  });

  it('does not expose technical errors when toggling a server fails at runtime', async () => {
    mockSdkStore.upsertServer.mockRejectedValueOnce(
      new Error('mount https://private.example failed with token=private'),
    );
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:config',
        mcp: { servers: [configServers[0]] },
        provenance: {},
      },
    } as any);
    render(<McpConfig instanceId={instanceId} />);

    fireEvent.click(screen.getByRole('switch', {
      name: 'Toggle server test-stdio enabled state',
    }));

    expect(await screen.findByText(
      'The MCP operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/private\.example|token=private/)).not.toBeInTheDocument();
  });

  it('does not expose technical errors when removing a server fails at runtime', async () => {
    mockSdkStore.removeServer.mockRejectedValueOnce(
      new Error('cleanup /secret/remove failed with token=private'),
    );
    mockUseSdkConfigStore.mockReturnValue({
      ...mockSdkStore,
      snapshot: {
        version: 1,
        revision: 'sha256:config',
        mcp: { servers: [configServers[0]] },
        provenance: {},
      },
    } as any);
    render(<McpConfig instanceId={instanceId} />);

    fireEvent.click(screen.getByTitle('Remove'));
    fireEvent.click(await screen.findByRole('button', { name: 'Yes' }));

    expect(await screen.findByText(
      'The MCP operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/remove|token=private/)).not.toBeInTheDocument();
  });
});
