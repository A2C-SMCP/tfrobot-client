import { render, screen, waitFor } from '../helpers/render';
import { ActivityViewer } from '@/components/ActivityViewer';
import { useActivityStore } from '@/stores/activityStore';

const activityStore = {
  items: [],
  total: 73,
  loading: false,
  error: null,
  query: { scope: { kind: 'all' }, limit: 50, offset: 0 },
  setQueryAndFetch: vi.fn().mockResolvedValue(undefined),
  fetchActivity: vi.fn().mockResolvedValue(undefined),
  exportActivity: vi.fn().mockResolvedValue(undefined),
  clearActivity: vi.fn().mockResolvedValue(undefined),
};

vi.mock('@/stores/activityStore', () => ({
  useActivityStore: vi.fn(() => activityStore),
}));

vi.mock('@/stores/computerStore', () => ({
  useComputerStore: vi.fn(() => ({
    instances: [{ id: 'computer-a', name: 'Computer A' }],
    fetchInstances: vi.fn().mockResolvedValue(undefined),
  })),
}));

describe('ActivityViewer', () => {
  beforeEach(() => vi.clearAllMocks());

  it('loads an explicit all-activity scope in global mode', async () => {
    render(<ActivityViewer />);

    await waitFor(() => expect(activityStore.setQueryAndFetch).toHaveBeenCalledWith({
      start_time: undefined,
      end_time: undefined,
      levels: undefined,
      categories: undefined,
      keyword: undefined,
      limit: 50,
      offset: 0,
      scope: { kind: 'all' },
    }));
    expect(screen.getByText('Activity')).toBeInTheDocument();
    expect(screen.getAllByLabelText('Activity scope').length).toBeGreaterThan(0);
  });

  it('uses a computer scope for an embedded viewer', async () => {
    render(<ActivityViewer instanceId="computer-a" />);

    await waitFor(() => expect(activityStore.setQueryAndFetch).toHaveBeenCalledWith(
      expect.objectContaining({ scope: { kind: 'computer', computer_id: 'computer-a' } }),
    ));
    expect(screen.queryAllByLabelText('Activity scope')).toHaveLength(0);
  });

  it('renders structured scope, outcome, and operation columns', () => {
    const populated = {
      ...activityStore,
      items: [
        {
          id: 1,
          timestamp: '2026-01-01T00:00:00Z',
          scope: { kind: 'computer', computer_id: 'deleted-computer' },
          level: 'error',
          category: 'mcp',
          event_type: 'server',
          operation: 'start',
          outcome: 'failed',
          message: 'Server failed',
        },
        {
          id: 2,
          timestamp: '2026-01-01T00:01:00Z',
          scope: { kind: 'client' },
          level: 'info',
          category: 'system',
          event_type: 'lifecycle',
          operation: 'start',
          outcome: 'succeeded',
          message: 'Client started',
        },
        {
          id: 3,
          timestamp: '2026-01-01T00:02:00Z',
          scope: { kind: 'client' },
          level: 'warn',
          category: 'system',
          event_type: 'legacy',
          operation: 'import',
          outcome: 'unknown',
          message: 'Legacy activity',
        },
      ],
    };
    vi.mocked(useActivityStore).mockReturnValue(populated as never);

    render(<ActivityViewer />);

    expect(screen.getByText('deleted-computer')).toBeInTheDocument();
    expect(screen.getAllByText('Outcome').length).toBeGreaterThan(0);
    expect(screen.getByText('FAILED')).toBeInTheDocument();
    expect(screen.getByText('SUCCEEDED')).toBeInTheDocument();
    expect(screen.getByText('UNKNOWN')).toBeInTheDocument();
    expect(screen.getAllByText('start').length).toBeGreaterThan(0);
    expect(screen.getByText('Server failed')).toBeInTheDocument();
  });
});
