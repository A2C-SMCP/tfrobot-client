import { invoke } from '@tauri-apps/api/core';
import { useActivityStore, type ActivityPage } from '@/stores/activityStore';

const mockedInvoke = vi.mocked(invoke);

describe('activityStore', () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
    useActivityStore.getState().reset();
  });

  it('uses the server total for pagination', async () => {
    const page: ActivityPage = {
      items: [{
        id: 1,
        timestamp: '2026-01-01T00:00:00Z',
        scope: { kind: 'client' },
        level: 'info',
        category: 'system',
        event_type: 'lifecycle',
        operation: 'start',
        outcome: 'succeeded',
        message: 'started',
      }],
      total: 42,
      limit: 50,
      offset: 0,
    };
    mockedInvoke.mockResolvedValueOnce(page);

    await useActivityStore.getState().fetchActivity();

    expect(mockedInvoke).toHaveBeenCalledWith('get_activity', {
      query: { scope: { kind: 'all' }, limit: 50, offset: 0 },
    });
    expect(useActivityStore.getState().items).toEqual(page.items);
    expect(useActivityStore.getState().total).toBe(42);
  });

  it('keeps only the latest overlapping request', async () => {
    let resolveFirst!: (page: ActivityPage) => void;
    const first = new Promise<ActivityPage>((resolve) => {
      resolveFirst = resolve;
    });
    mockedInvoke
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce({ items: [], total: 0, limit: 50, offset: 0 });

    const older = useActivityStore.getState().setQueryAndFetch({ keyword: 'old' });
    const newer = useActivityStore.getState().setQueryAndFetch({ keyword: 'new' });
    await newer;
    resolveFirst({ items: [], total: 99, limit: 50, offset: 0 });
    await older;

    expect(useActivityStore.getState().query.keyword).toBe('new');
    expect(useActivityStore.getState().total).toBe(0);
  });
});
