import { invoke } from '@tauri-apps/api/core';
import { activityViewStore, pruneActivityViews, resetActivityViews, useActivityStore, type ActivityPage } from '@/stores/activityStore';

describe('activity navigation isolation', () => {
  beforeEach(() => { resetActivityViews(); vi.mocked(invoke).mockReset(); });

  it('keeps global and each Computer query and pagination independent', async () => {
    vi.mocked(invoke).mockResolvedValue({ items: [], total: 200, limit: 20, offset: 40 });
    const a = activityViewStore('a');
    const b = activityViewStore('b');
    await useActivityStore.getState().setQueryAndFetch({ keyword: 'global', offset: 100 });
    await a.getState().setQueryAndFetch({ keyword: 'alpha', offset: 40 });
    await b.getState().setQueryAndFetch({ keyword: 'beta', offset: 20 });
    expect(useActivityStore.getState().query).toMatchObject({ keyword: 'global', offset: 100 });
    expect(a.getState().query).toMatchObject({ keyword: 'alpha', offset: 40, scope: { computer_id: 'a' } });
    expect(b.getState().query).toMatchObject({ keyword: 'beta', offset: 20, scope: { computer_id: 'b' } });
  });

  it('rejects stale responses after reset, deletion and re-creation of the same context', async () => {
    let resolve!: (page: ActivityPage) => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise<ActivityPage>((done) => { resolve = done; }));
    const old = activityViewStore('a');
    const pending = old.getState().fetchActivity();
    pruneActivityViews(new Set());
    const next = activityViewStore('a');
    vi.mocked(invoke).mockResolvedValue({ items: [], total: 2, limit: 50, offset: 0 });
    await next.getState().fetchActivity();
    resolve({ items: [], total: 999, limit: 50, offset: 0 });
    await pending;
    expect(old.getState().total).toBe(0);
    expect(next.getState().total).toBe(2);
    expect(old).not.toBe(next);
  });

  it('does not reuse a pre-reset request id in the global view', async () => {
    let resolve!: (page: ActivityPage) => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise<ActivityPage>((done) => { resolve = done; }));
    const pending = useActivityStore.getState().fetchActivity();
    resetActivityViews();
    vi.mocked(invoke).mockResolvedValue({ items: [], total: 2, limit: 50, offset: 0 });
    await useActivityStore.getState().fetchActivity();
    resolve({ items: [], total: 999, limit: 50, offset: 0 });
    await pending;
    expect(useActivityStore.getState().total).toBe(2);
  });
});
