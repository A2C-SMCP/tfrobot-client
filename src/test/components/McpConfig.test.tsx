import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { McpConfig } from '@/components/McpConfig';

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

vi.mock('@/stores/sdkConfigStore', () => ({
  useSdkConfigStore: vi.fn(() => mockSdkStore),
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
    mockSdkStore.upsertServer.mockResolvedValue(undefined);
    mockUseSdkConfigStore.mockReturnValue({ ...mockSdkStore } as any);
  });

  it('loads the SDK config snapshot and validation on mount', () => {
    render(<McpConfig instanceId={instanceId} />);
    expect(mockSdkStore.fetchConfig).toHaveBeenCalledWith(instanceId);
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

  it('renders error alert when error exists', () => {
    mockUseSdkConfigStore.mockReturnValue({ ...mockSdkStore, error: 'Something broke' } as any);
    render(<McpConfig instanceId={instanceId} />);
    expect(screen.getByText('Something broke')).toBeInTheDocument();
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

  it('persists a declaration through config CRUD without requesting runtime inputs', async () => {
    render(<McpConfig instanceId={instanceId} />);
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Submit mocked server' }));

    await waitFor(() => expect(mockSdkStore.upsertServer).toHaveBeenCalledOnce());
    expect(mockSdkStore.upsertServer).toHaveBeenCalledWith(instanceId, expect.objectContaining({
      name: 'runtime-server',
    }));
    expect(screen.queryByText('Secret required to start')).not.toBeInTheDocument();
  }, 10000);
});
