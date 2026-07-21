import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { McpRuntimeControls } from '@/components/McpConfig/McpRuntimeControls';

const mockStore = {
  servers: [{
    bundleId: 'runtime-server-id',
    name: 'runtime-server',
    running: false,
    status_message: 'Stopped',
    disabled: false,
    managedBy: { type: 'user' },
  }],
  loading: false,
  error: null,
  fetchServers: vi.fn(),
  startServer: vi.fn(),
  stopServer: vi.fn(),
  startAll: vi.fn(),
  stopAll: vi.fn(),
};

vi.mock('@/stores/mcpStore', () => ({
  useMcpStore: vi.fn(() => mockStore),
}));

describe('McpRuntimeControls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('loads runtime status and exposes only lifecycle actions', () => {
    render(<McpRuntimeControls instanceId="computer-a" />);

    expect(mockStore.fetchServers).toHaveBeenCalledWith('computer-a');
    expect(screen.getByText('MCP Runtime')).toBeInTheDocument();
    expect(screen.getByText('runtime-server')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Start All$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Stop All$/ })).toBeInTheDocument();
    expect(screen.queryByText('Add Server')).not.toBeInTheDocument();
    expect(screen.queryByText('Import Config')).not.toBeInTheDocument();
    expect(screen.queryByText('Validate Schema')).not.toBeInTheDocument();
  });

  it('disables lifecycle actions when the runtime cannot manage MCP servers', () => {
    render(<McpRuntimeControls instanceId="computer-a" disabled />);

    expect(screen.getByRole('button', { name: /Start All$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Stop All$/ })).toBeDisabled();
    expect(screen.getByTitle('Start')).toBeDisabled();
  });

  it('starts a server through the runtime action path', async () => {
    mockStore.startServer.mockResolvedValueOnce(undefined);
    render(<McpRuntimeControls instanceId="computer-a" />);

    fireEvent.click(screen.getByTitle('Start'));

    await waitFor(() => {
      expect(mockStore.startServer).toHaveBeenCalledWith('computer-a', 'runtime-server-id');
    });
    expect(await screen.findByText('Server runtime-server started')).toBeInTheDocument();
  });
});
