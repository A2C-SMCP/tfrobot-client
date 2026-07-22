import { render, screen, fireEvent } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { Dashboard } from '@/components/Dashboard';
import { runtimeSnapshot } from '../helpers/store';

const mockFetchDashboard = vi.fn();

vi.mock('@/stores/dashboardStore', () => ({
  useDashboardStore: vi.fn(() => ({
    data: null,
    loading: false,
    fetchDashboard: mockFetchDashboard,
  })),
}));

const mockSelectInstance = vi.fn();

vi.mock('@/stores/computerStore', () => ({
  useComputerStore: vi.fn(() => ({
    selectInstance: mockSelectInstance,
  })),
}));

// Access the mocked module for dynamic return values
import { useDashboardStore } from '@/stores/dashboardStore';
const mockUseDashboardStore = vi.mocked(useDashboardStore);

const mockDashboardData = {
  computer_total: 2,
  computer_running: 1,
  computer_stopped: 1,
  computer_connected: 1,
  computers: [
    {
      id: 'computer-a',
      name: 'Computer A',
      running: true,
      runtime: runtimeSnapshot({
        lifecycle: 'connected',
        config_revision: 2,
        mcp_servers: 5,
        active_mcp_servers: 4,
        capability_revision: 3,
        tools: 9,
        skills: 4,
      }),
      connected: true,
      mcp_server_count: 5,
      robot_name: 'Robot A',
      connection_profile: 'prod',
    },
    {
      id: 'computer-b',
      name: 'Computer B',
      running: false,
      runtime: runtimeSnapshot({
        lifecycle: 'shutdown',
        mcp_servers: 2,
      }),
      connected: false,
      mcp_server_count: 2,
    },
  ],
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

    expect(screen.getByText('Computer A')).toBeInTheDocument();
    expect(screen.getByText('Computer B')).toBeInTheDocument();
    expect(screen.getByText('Robot A')).toBeInTheDocument();
    expect(screen.getAllByText('Connection failed').length).toBeGreaterThan(0);
    expect(screen.getByText('Node.js')).toBeInTheDocument();
    expect(screen.getByText('Python')).toBeInTheDocument();
    expect(screen.getByText('Config Revision: 2')).toBeInTheDocument();
    expect(screen.getByText('Capability Revision: 3')).toBeInTheDocument();
    expect(screen.getByText('Tools: 9')).toBeInTheDocument();
    expect(screen.getByText('Skills: 4')).toBeInTheDocument();
  });

  it('shows runtime availability status', () => {
    mockUseDashboardStore.mockReturnValue({
      data: mockDashboardData,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    render(<Dashboard onNavigate={mockOnNavigate} />);
    expect(screen.getByText('Available')).toBeInTheDocument();
    expect(screen.getByText('Unavailable')).toBeInTheDocument();
  });

  it('shows correct computer counts', () => {
    mockUseDashboardStore.mockReturnValue({
      data: mockDashboardData,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    const { container } = render(<Dashboard onNavigate={mockOnNavigate} />);
    const text = container.textContent || '';
    expect(text).toContain('Running1');
    expect(text).toContain('Stopped1');
    expect(text).toContain('Connected1');
  });

  it('renders log entries with level tags', () => {
    mockUseDashboardStore.mockReturnValue({
      data: mockDashboardData,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    render(<Dashboard onNavigate={mockOnNavigate} />);
    expect(screen.getByText('Server started')).toBeInTheDocument();
    expect(screen.getAllByText('Connection failed').length).toBeGreaterThan(0);
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

  it('opens a selected Computer overview from the shortcut list', () => {
    mockUseDashboardStore.mockReturnValue({
      data: mockDashboardData,
      loading: false,
      fetchDashboard: mockFetchDashboard,
    } as any);
    render(<Dashboard onNavigate={mockOnNavigate} />);

    fireEvent.click(screen.getAllByText('Open Details')[0]);

    expect(mockSelectInstance).toHaveBeenCalledWith('computer-a');
    expect(mockOnNavigate).toHaveBeenCalledWith('computer-detail:overview');
  });
});
