import { render, screen } from '../helpers/render';
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

vi.mock('@/stores/mcpStore', () => ({
  useMcpStore: vi.fn(() => mockStore),
}));

import { useMcpStore } from '@/stores/mcpStore';
const mockUseMcpStore = vi.mocked(useMcpStore);

const mockServers = [
  { name: 'test-stdio', running: true, status_message: 'Running', disabled: false },
  { name: 'test-http', running: false, status_message: 'Stopped', disabled: false },
];

describe('McpConfig', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseMcpStore.mockReturnValue({ ...mockStore, servers: [] } as any);
  });

  it('calls fetchServers on mount', () => {
    render(<McpConfig />);
    expect(mockStore.fetchServers).toHaveBeenCalled();
  });

  it('renders title and action buttons', () => {
    render(<McpConfig />);
    expect(screen.getByText('MCP Servers')).toBeInTheDocument();
    expect(screen.getByText('Add Server')).toBeInTheDocument();
    expect(screen.getByText('Start All')).toBeInTheDocument();
    expect(screen.getByText('Stop All')).toBeInTheDocument();
    expect(screen.getByText('Import Config')).toBeInTheDocument();
    expect(screen.getByText('Export Config')).toBeInTheDocument();
  });

  it('renders error alert when error exists', () => {
    mockUseMcpStore.mockReturnValue({ ...mockStore, error: 'Something broke' } as any);
    render(<McpConfig />);
    expect(screen.getByText('Something broke')).toBeInTheDocument();
  });

  it('renders server list with servers', () => {
    mockUseMcpStore.mockReturnValue({ ...mockStore, servers: mockServers } as any);
    render(<McpConfig />);
    expect(screen.getByText('test-stdio')).toBeInTheDocument();
    expect(screen.getByText('test-http')).toBeInTheDocument();
  });
});
