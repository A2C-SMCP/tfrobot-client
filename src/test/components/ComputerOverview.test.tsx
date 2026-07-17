import { render, screen, fireEvent } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { ComputerOverview } from '@/components/Computer/ComputerOverview';
import { useComputerOverviewStore } from '@/stores/computerOverviewStore';
import { runtimeSnapshot } from '../helpers/store';

const mockFetchOverview = vi.fn();
const mockOpenTab = vi.fn();

vi.mock('@/stores/computerOverviewStore', () => ({
  useComputerOverviewStore: vi.fn(() => ({
    data: null,
    loading: false,
    error: null,
    fetchOverview: mockFetchOverview,
  })),
}));

const mockUseComputerOverviewStore = vi.mocked(useComputerOverviewStore);

const mockOverviewData = {
  id: 'computer-a',
  name: 'Computer A',
  running: true,
  runtime: runtimeSnapshot({
    lifecycle: 'connected',
    mcp_servers: 3,
    active_mcp_servers: 2,
    tools: 10,
    skills: 4,
  }),
  connected: true,
  connection_url: 'https://smcp.example.com',
  connection_profile: 'prod',
  robot_name: 'Robot A',
  mcp_total: 3,
  mcp_running: 2,
  mcp_stopped: 1,
  tools_count: 10,
  recent_logs: [
    { id: 1, timestamp: '2025-01-01T10:00:00Z', level: 'info', category: 'mcp', message: 'Server started' },
  ],
};

describe('ComputerOverview', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseComputerOverviewStore.mockReturnValue({
      data: mockOverviewData,
      loading: false,
      error: null,
      fetchOverview: mockFetchOverview,
    } as any);
  });

  it('fetches overview on mount and renders instance metrics', () => {
    render(<ComputerOverview instanceId="computer-a" onOpenTab={mockOpenTab} />);

    expect(mockFetchOverview).toHaveBeenCalledWith('computer-a');
    expect(screen.getAllByText('Connected').length).toBeGreaterThan(0);
    expect(screen.getByText('prod — https://smcp.example.com')).toBeInTheDocument();
    expect(screen.getByText('Bound robot: Robot A')).toBeInTheDocument();
    expect(screen.getByText('10')).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Skills 4' })).toBeInTheDocument();
    expect(screen.getByText('Server started')).toBeInTheDocument();
  });

  it('opens detail tabs from overview actions', () => {
    render(<ComputerOverview instanceId="computer-a" onOpenTab={mockOpenTab} />);

    fireEvent.click(screen.getByText('Robot Connection'));
    fireEvent.click(screen.getAllByText('MCP Servers')[1]);
    fireEvent.click(screen.getByText('Debug Panel'));
    fireEvent.click(screen.getAllByText('Logs')[1]);

    expect(mockOpenTab).toHaveBeenNthCalledWith(1, 'connection');
    expect(mockOpenTab).toHaveBeenNthCalledWith(2, 'mcp');
    expect(mockOpenTab).toHaveBeenNthCalledWith(3, 'debug');
    expect(mockOpenTab).toHaveBeenNthCalledWith(4, 'logs');
  });

  it('shows loading when data for this instance is not available', () => {
    mockUseComputerOverviewStore.mockReturnValue({
      data: null,
      loading: true,
      error: null,
      fetchOverview: mockFetchOverview,
    } as any);

    const { container } = render(<ComputerOverview instanceId="computer-a" onOpenTab={mockOpenTab} />);

    expect(container.querySelector('.ant-spin')).toBeInTheDocument();
  });
});
