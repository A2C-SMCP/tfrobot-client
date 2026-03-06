import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { LogViewer } from '@/components/LogViewer';

const mockStore = {
  logs: [],
  loading: false,
  error: null,
  filter: {},
  setFilter: vi.fn(),
  fetchLogs: vi.fn(),
  exportLogs: vi.fn(),
  clearLogs: vi.fn(),
};

vi.mock('@/stores/logStore', () => ({
  useLogStore: vi.fn(() => mockStore),
}));

import { useLogStore } from '@/stores/logStore';
const mockUseLogStore = vi.mocked(useLogStore);

const mockLogs = [
  { id: 1, timestamp: '2025-01-01T10:00:00Z', level: 'info', category: 'system', message: 'App started', details: null },
  { id: 2, timestamp: '2025-01-01T10:01:00Z', level: 'error', category: 'mcp', message: 'Server crashed', details: 'stack trace' },
  { id: 3, timestamp: '2025-01-01T10:02:00Z', level: 'warn', category: 'connection', message: 'Reconnecting' },
];

describe('LogViewer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseLogStore.mockReturnValue({ ...mockStore } as any);
  });

  it('calls fetchLogs on mount', () => {
    render(<LogViewer />);
    expect(mockStore.fetchLogs).toHaveBeenCalledOnce();
  });

  it('renders title', () => {
    render(<LogViewer />);
    expect(screen.getByText('Logs')).toBeInTheDocument();
  });

  it('renders filter controls', () => {
    render(<LogViewer />);
    // Time presets
    expect(screen.getByText('1h')).toBeInTheDocument();
    expect(screen.getByText('6h')).toBeInTheDocument();
    expect(screen.getByText('24h')).toBeInTheDocument();
    expect(screen.getByText('7d')).toBeInTheDocument();
    // Action buttons
    expect(screen.getByText('Refresh')).toBeInTheDocument();
    expect(screen.getByText('Export')).toBeInTheDocument();
    expect(screen.getByText('Clear')).toBeInTheDocument();
  });

  it('renders log entries', () => {
    mockUseLogStore.mockReturnValue({ ...mockStore, logs: mockLogs } as any);
    render(<LogViewer />);
    expect(screen.getByText('App started')).toBeInTheDocument();
    expect(screen.getByText('Server crashed')).toBeInTheDocument();
    expect(screen.getByText('Reconnecting')).toBeInTheDocument();
  });

  it('renders log level tags', () => {
    mockUseLogStore.mockReturnValue({ ...mockStore, logs: mockLogs } as any);
    render(<LogViewer />);
    expect(screen.getByText('INFO')).toBeInTheDocument();
    expect(screen.getByText('ERROR')).toBeInTheDocument();
    expect(screen.getByText('WARN')).toBeInTheDocument();
  });
});
