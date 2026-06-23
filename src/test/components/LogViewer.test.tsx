import { render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { LogViewer } from '@/components/LogViewer';

const mockStore = {
  logs: [],
  loading: false,
  error: null,
  filter: { limit: 50, offset: 0 },
  setFilter: vi.fn().mockResolvedValue(undefined),
  setFilterAndFetch: vi.fn().mockResolvedValue(undefined),
  fetchLogs: vi.fn().mockResolvedValue(undefined),
  exportLogs: vi.fn().mockResolvedValue(undefined),
  clearLogs: vi.fn().mockResolvedValue(undefined),
};

vi.mock('@/stores/logStore', () => ({
  useLogStore: vi.fn(() => mockStore),
}));

const mockComputerStore = {
  instances: [
    { id: 'computer-a', name: 'Computer A' },
    { id: 'computer-b', name: 'Computer B' },
  ],
  fetchInstances: vi.fn().mockResolvedValue(undefined),
};

vi.mock('@/stores/computerStore', () => ({
  useComputerStore: vi.fn(() => mockComputerStore),
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

  it('calls fetchLogs on mount', async () => {
    render(<LogViewer />);
    await waitFor(() => {
      expect(mockStore.setFilterAndFetch).toHaveBeenCalledWith({
        start_time: undefined,
        end_time: undefined,
        levels: undefined,
        categories: undefined,
        keyword: undefined,
        computer_instance_id: undefined,
        limit: 50,
        offset: 0,
      });
    });
  });

  it('clears stale computer filter in global mode', async () => {
    render(<LogViewer />);
    await waitFor(() => {
      expect(mockStore.setFilterAndFetch).toHaveBeenCalledWith({
        start_time: undefined,
        end_time: undefined,
        levels: undefined,
        categories: undefined,
        keyword: undefined,
        computer_instance_id: undefined,
        limit: 50,
        offset: 0,
      });
    });
  });

  it('sets computer filter and hides global clear in scoped mode', async () => {
    render(<LogViewer instanceId="computer-a" />);
    await waitFor(() => {
      expect(mockStore.setFilterAndFetch).toHaveBeenCalledWith({
        start_time: undefined,
        end_time: undefined,
        levels: undefined,
        categories: undefined,
        keyword: undefined,
        computer_instance_id: 'computer-a',
        limit: 50,
        offset: 0,
      });
    });
    expect(screen.queryByText('Clear')).not.toBeInTheDocument();
  });

  it('does not carry stale global filters into scoped mode', async () => {
    mockUseLogStore.mockReturnValue({
      ...mockStore,
      filter: {
        limit: 20,
        offset: 40,
        keyword: 'old keyword',
        levels: ['error'],
        categories: ['mcp'],
        start_time: '2026-01-01T00:00:00Z',
        computer_instance_id: 'computer-b',
      },
    } as any);

    render(<LogViewer instanceId="computer-a" />);

    await waitFor(() => {
      expect(mockStore.setFilterAndFetch).toHaveBeenCalledWith({
        start_time: undefined,
        end_time: undefined,
        levels: undefined,
        categories: undefined,
        keyword: undefined,
        computer_instance_id: 'computer-a',
        limit: 50,
        offset: 0,
      });
    });
  });

  it('renders filter controls from the current store filter', () => {
    mockUseLogStore.mockReturnValue({
      ...mockStore,
      filter: {
        limit: 50,
        offset: 0,
        keyword: 'failed call',
        levels: ['error'],
        categories: ['tool'],
      },
    } as any);

    render(<LogViewer />);

    expect(screen.getByDisplayValue('failed call')).toBeInTheDocument();
    expect(screen.getByText('ERROR')).toBeInTheDocument();
    expect(screen.getByText('tool')).toBeInTheDocument();
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
