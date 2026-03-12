import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { Dashboard } from '@/components/Dashboard';

const mockFetchDashboard = vi.fn();

vi.mock('@/stores/dashboardStore', () => ({
  useDashboardStore: vi.fn(() => ({
    data: null,
    loading: false,
    fetchDashboard: mockFetchDashboard,
  })),
}));

// Access the mocked module for dynamic return values
import { useDashboardStore } from '@/stores/dashboardStore';
const mockUseDashboardStore = vi.mocked(useDashboardStore);

const mockDashboardData = {
  connected: true,
  connection_url: 'https://smcp.example.com',
  connection_profile: 'prod',
  mcp_total: 5,
  mcp_running: 3,
  mcp_stopped: 2,
  tools_count: 12,
  recent_logs: [
    { id: 1, timestamp: '2025-01-01T10:00:00Z', level: 'info', category: 'system', message: 'Server started' },
    { id: 2, timestamp: '2025-01-01T10:01:00Z', level: 'error', category: 'mcp', message: 'Connection failed' },
  ],
  runtimes: [
    { name: 'Node.js', path: '/usr/local/bin/node', available: true },
    { name: 'Python', path: undefined, available: false },
  ],
};

const mockOnNavigate = vi.fn();

describe('Dashboard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseDashboardStore.mockReturnValue({
      data: null,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
  });

  it('calls fetchDashboard on mount', () => {
    render(<Dashboard onNavigate={mockOnNavigate} />);
    expect(mockFetchDashboard).toHaveBeenCalledOnce();
  });

  it('shows spinner while loading with no data', () => {
    mockUseDashboardStore.mockReturnValue({
      data: null,
      loading: true,
      fetchDashboard: mockFetchDashboard,
    } as any);
    const { container } = render(<Dashboard onNavigate={mockOnNavigate} />);
    expect(container.querySelector('.ant-spin')).toBeInTheDocument();
  });

  it('renders nothing when data is null and not loading', () => {
    const { container } = render(<Dashboard onNavigate={mockOnNavigate} />);
    // After loading=false and data=null, should render null (empty)
    expect(container.querySelector('.ant-card')).not.toBeInTheDocument();
  });

  it('renders all cards when data is loaded', () => {
    mockUseDashboardStore.mockReturnValue({
      data: mockDashboardData,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    render(<Dashboard onNavigate={mockOnNavigate} />);

    // Connection card
    expect(screen.getByText('Connected')).toBeInTheDocument();
    // MCP card
    expect(screen.getByText(/5/)).toBeInTheDocument();
    // Tools count
    expect(screen.getByText('12')).toBeInTheDocument();
    // Runtimes
    expect(screen.getByText('Node.js')).toBeInTheDocument();
    expect(screen.getByText('Python')).toBeInTheDocument();
  });

  it('shows disconnected badge when not connected', () => {
    mockUseDashboardStore.mockReturnValue({
      data: { ...mockDashboardData, connected: false, connection_profile: undefined },
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    render(<Dashboard onNavigate={mockOnNavigate} />);
    expect(screen.getByText('Disconnected')).toBeInTheDocument();
  });

  it('shows correct server counts', () => {
    mockUseDashboardStore.mockReturnValue({
      data: mockDashboardData,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    const { container } = render(<Dashboard onNavigate={mockOnNavigate} />);
    // Verify running and stopped counts exist in the rendered output
    const text = container.textContent || '';
    expect(text).toContain('3');
    expect(text).toContain('2');
  });

  it('renders log entries with level tags', () => {
    mockUseDashboardStore.mockReturnValue({
      data: mockDashboardData,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    render(<Dashboard onNavigate={mockOnNavigate} />);
    expect(screen.getByText('Server started')).toBeInTheDocument();
    expect(screen.getByText('Connection failed')).toBeInTheDocument();
  });

  it('renders empty activity when no logs', () => {
    mockUseDashboardStore.mockReturnValue({
      data: { ...mockDashboardData, recent_logs: [] },
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    render(<Dashboard onNavigate={mockOnNavigate} />);
    expect(screen.getByText('No recent activity')).toBeInTheDocument();
  });
});
