import { PageHost } from '@/components/Navigation/PageHost';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { ActivityViewer } from '@/components/ActivityViewer';
import { ACTIVITY_CATEGORIES } from '@/components/ActivityViewer/categories';
import { useActivityStore, activityViewStore, resetActivityViews } from '@/stores/activityStore';
import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = vi.mocked(invoke);

vi.mock('@/stores/computerStore', () => ({
  useComputerStore: vi.fn(() => ({
    instances: [{ id: 'computer-a', name: 'Computer A' }],
    fetchInstances: vi.fn().mockResolvedValue(undefined),
  })),
}));

describe('ActivityViewer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetActivityViews();
    mockedInvoke.mockResolvedValue({ items: [], total: 73, limit: 50, offset: 0 });
  });

  it('loads an explicit all-activity scope in global mode', async () => {
    render(<ActivityViewer />);

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('get_activity', { query: {
      start_time: undefined,
      end_time: undefined,
      levels: undefined,
      categories: undefined,
      keyword: undefined,
      limit: 50,
      offset: 0,
      scope: { kind: 'all' },
    } }));
    expect(screen.getByText('Activity')).toBeInTheDocument();
    expect(screen.getAllByLabelText('Activity scope').length).toBeGreaterThan(0);
  });

  it('refreshes on return without resetting filters or pagination', async () => {
    const tree = (active: boolean) => <PageHost name="logs" active={active}><ActivityViewer /></PageHost>;
    const view = render(tree(true));
    await waitFor(() => expect(useActivityStore.getState().initialized).toBe(true));
    await act(async () => { await useActivityStore.getState().setQueryAndFetch({ keyword: 'mcp', offset: 50 }); });
    view.rerender(tree(false));
    mockedInvoke.mockClear();
    view.rerender(tree(true));
    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('get_activity', {
      query: expect.objectContaining({ keyword: 'mcp', offset: 50 }),
    }));
    expect(useActivityStore.getState().query).toMatchObject({ keyword: 'mcp', offset: 50 });
  });

  it('revalidates after a clear invalidates a still-pending activity request', async () => {
    let finish!: (value: unknown) => void;
    let globalReads = 0;
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === 'clear_activity') return;
      const scope = (args as { query: { scope: { kind: string } } }).query.scope;
      if (scope.kind === 'all' && ++globalReads === 1) return new Promise((resolve) => { finish = resolve; });
      return { items: [], total: 3, limit: 50, offset: 0 };
    });
    render(<ActivityViewer />);
    await waitFor(() => expect(finish).toBeDefined());
    await act(async () => { await activityViewStore('computer-a').getState().clearActivity(); });
    expect(useActivityStore.getState()).toMatchObject({ loading: true, invalidated: true });
    await act(async () => { finish({ items: [], total: 99, limit: 50, offset: 0 }); });
    await waitFor(() => expect(globalReads).toBe(2));
    expect(useActivityStore.getState().total).toBe(3);
  });

  it('uses a computer scope for an embedded viewer', async () => {
    render(<ActivityViewer instanceId="computer-a" />);

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('get_activity', { query:
      expect.objectContaining({ scope: { kind: 'computer', computer_id: 'computer-a' } }) },
    ));
    expect(screen.queryAllByLabelText('Activity scope')).toHaveLength(0);
  });

  it('renders structured scope, outcome, and operation columns', () => {
    const populated = {
      initialized: true,
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
          correlation_id: 'request-1',
          fields: {
            trigger: 'client_control',
            provider: 'built_in_mcp',
            bundle_id: 'client_control',
          },
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
    useActivityStore.setState(populated as never);

    render(<ActivityViewer />);

    expect(screen.getByText('deleted-computer')).toBeInTheDocument();
    expect(screen.getAllByText('Outcome').length).toBeGreaterThan(0);
    expect(screen.getByText('FAILED')).toBeInTheDocument();
    expect(screen.getByText('SUCCEEDED')).toBeInTheDocument();
    expect(screen.getByText('UNKNOWN')).toBeInTheDocument();
    expect(screen.getAllByText('start').length).toBeGreaterThan(0);
    expect(screen.getByText('Server failed')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Expand row' }));
    expect(screen.getByText((_, element) => (
      element?.tagName === 'PRE'
      && element.textContent?.includes('"correlation_id": "request-1"') === true
      && element.textContent?.includes('"provider": "built_in_mcp"') === true
      && element.textContent?.includes('"bundle_id": "client_control"') === true
    ))).toBeInTheDocument();
  });

  it('offers every Computer activity domain without a Client Control category', () => {
    expect(ACTIVITY_CATEGORIES).toEqual(expect.arrayContaining([
      'computer', 'runtime', 'connection', 'mcp', 'input',
      'tool', 'resource', 'skill', 'marketplace',
    ]));
    expect(ACTIVITY_CATEGORIES).not.toContain('client_control');
  });

  it('applies a selected Computer category to the activity query', async () => {
    render(<ActivityViewer instanceId="computer-a" />);

    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Activity category' }));
    fireEvent.click(await screen.findByText('runtime'));

    await waitFor(() => expect(activityViewStore('computer-a').getState().query.categories).toEqual(['runtime']));
  });
});
