import { waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import {
  IMPRESSION_FLUSH_THRESHOLD,
  installUiNoticeFlushHooks,
  useUiNoticeStore,
} from '@/stores/uiNoticeStore';

const mockedInvoke = vi.mocked(invoke);

describe('uiNoticeStore', () => {
  beforeEach(() => {
    useUiNoticeStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('reports only the notices the user actually dismissed', async () => {
    mockedInvoke.mockResolvedValueOnce({
      schemaVersion: 1,
      entries: {
        'mcp-runtime-keychain': {
          firstSeenAt: 1,
          dismissedAt: 2,
          impressions: 3,
          helpClicks: 0,
        },
      },
    });

    await useUiNoticeStore.getState().fetch();

    expect(useUiNoticeStore.getState().isDismissed('mcp-runtime-keychain')).toBe(true);
    expect(useUiNoticeStore.getState().isDismissed('remote-control-security')).toBe(false);
  });

  it('fails open when the persisted state cannot be read', async () => {
    mockedInvoke.mockRejectedValueOnce(new Error('settings unavailable'));

    await useUiNoticeStore.getState().fetch();

    const state = useUiNoticeStore.getState();
    expect(state.loaded).toBe(true);
    expect(state.error).toContain('settings unavailable');
    expect(state.isDismissed('remote-control-security')).toBe(false);
  });

  it('batches impressions instead of writing on every view', async () => {
    for (let index = 0; index < IMPRESSION_FLUSH_THRESHOLD - 1; index += 1) {
      useUiNoticeStore.getState().recordImpression('mcp-runtime-keychain');
    }
    expect(mockedInvoke).not.toHaveBeenCalled();

    mockedInvoke.mockResolvedValueOnce({ schemaVersion: 1, entries: {} });
    useUiNoticeStore.getState().recordImpression('mcp-runtime-keychain');

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('update_ui_notice_state', {
      updates: {
        'mcp-runtime-keychain': {
          impressionsDelta: IMPRESSION_FLUSH_THRESHOLD,
          helpClicksDelta: 0,
        },
      },
    }));
  });

  it('flushes a pending impression together with an explicit dismissal', async () => {
    mockedInvoke.mockResolvedValueOnce({
      schemaVersion: 1,
      entries: {
        'remote-control-security': {
          firstSeenAt: 1,
          dismissedAt: 2,
          impressions: 1,
          helpClicks: 0,
        },
      },
    });
    useUiNoticeStore.getState().recordImpression('remote-control-security');

    await useUiNoticeStore.getState().dismiss('remote-control-security');

    expect(mockedInvoke).toHaveBeenCalledWith('update_ui_notice_state', {
      updates: {
        'remote-control-security': {
          impressionsDelta: 1,
          helpClicksDelta: 0,
          dismissed: true,
        },
      },
    });
    expect(useUiNoticeStore.getState().isDismissed('remote-control-security')).toBe(true);
  });

  it('restores the previous state when a dismissal cannot be persisted', async () => {
    mockedInvoke.mockRejectedValueOnce(new Error('offline'));

    await useUiNoticeStore.getState().dismiss('remote-control-security');

    expect(useUiNoticeStore.getState().isDismissed('remote-control-security')).toBe(false);
    expect(useUiNoticeStore.getState().error).toContain('offline');
  });

  it('flushes batched impressions when the window is hidden', async () => {
    const originalVisibility = Object.getOwnPropertyDescriptor(document, 'visibilityState');
    const uninstall = installUiNoticeFlushHooks();
    mockedInvoke.mockResolvedValueOnce({ schemaVersion: 1, entries: {} });
    useUiNoticeStore.getState().recordImpression('mcp-runtime-keychain');

    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('update_ui_notice_state', {
      updates: {
        'mcp-runtime-keychain': { impressionsDelta: 1, helpClicksDelta: 0 },
      },
    }));
    if (originalVisibility) {
      Object.defineProperty(document, 'visibilityState', originalVisibility);
    }
    uninstall();
  });

  it('re-batches impressions that a failed flush could not persist', async () => {
    mockedInvoke.mockRejectedValueOnce(new Error('offline'));
    useUiNoticeStore.getState().recordImpression('mcp-runtime-keychain');

    await useUiNoticeStore.getState().flushPending();
    expect(useUiNoticeStore.getState().error).toContain('offline');

    mockedInvoke.mockResolvedValueOnce({ schemaVersion: 1, entries: {} });
    await useUiNoticeStore.getState().flushPending();

    expect(mockedInvoke).toHaveBeenLastCalledWith('update_ui_notice_state', {
      updates: {
        'mcp-runtime-keychain': { impressionsDelta: 1, helpClicksDelta: 0 },
      },
    });
  });

  it('keeps a dismissal that lands while an earlier write response is still in flight', async () => {
    let resolveFlush!: (value: unknown) => void;
    mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => {
      resolveFlush = resolve;
    }));
    useUiNoticeStore.getState().recordImpression('mcp-runtime-keychain');
    const flush = useUiNoticeStore.getState().flushPending();

    // The dismissal is persisted first, then the older flush response arrives without it.
    mockedInvoke.mockResolvedValueOnce({
      schemaVersion: 1,
      entries: {
        'remote-control-security': {
          firstSeenAt: 1,
          dismissedAt: 2,
          impressions: 1,
          helpClicks: 0,
        },
      },
    });
    await useUiNoticeStore.getState().dismiss('remote-control-security');
    resolveFlush({ schemaVersion: 1, entries: {} });
    await flush;

    expect(useUiNoticeStore.getState().isDismissed('remote-control-security')).toBe(true);
  });
});

describe('notice event concurrency', () => {
  beforeEach(() => {
    useUiNoticeStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('rolls back only the failed dismissal while retaining another successful dismissal', async () => {
    let failFirst!: (reason: Error) => void;
    mockedInvoke.mockImplementationOnce(() => new Promise((_, reject) => { failFirst = reject; }));
    const store = useUiNoticeStore.getState();
    const first = store.dismiss('mcp-runtime-keychain');
    mockedInvoke.mockResolvedValueOnce({ entries: {
      'remote-control-security': { firstSeenAt: 1, dismissedAt: 2, impressions: 0, helpClicks: 0 },
    } });
    await store.dismiss('remote-control-security');
    failFirst(new Error('write failed'));
    await first;
    expect(store.isDismissed('mcp-runtime-keychain')).toBe(false);
    expect(store.isDismissed('remote-control-security')).toBe(true);
  });

  it('keeps buffered and in-flight counts visible without double counting on acknowledgement', async () => {
    let finish!: (value: unknown) => void;
    mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const store = useUiNoticeStore.getState();
    store.recordImpression('mcp-runtime-keychain');
    const flush = store.flushPending();
    store.recordImpression('mcp-runtime-keychain');
    expect(useUiNoticeStore.getState().entries['mcp-runtime-keychain'].impressions).toBe(2);
    finish({ entries: { 'mcp-runtime-keychain': {
      firstSeenAt: 1, dismissedAt: null, impressions: 1, helpClicks: 0,
    } } });
    await flush;
    expect(useUiNoticeStore.getState().entries['mcp-runtime-keychain'].impressions).toBe(2);
    mockedInvoke.mockResolvedValueOnce({ entries: { 'mcp-runtime-keychain': {
      firstSeenAt: 1, dismissedAt: null, impressions: 2, helpClicks: 0,
    } } });
    await store.flushPending();
    expect(useUiNoticeStore.getState().entries['mcp-runtime-keychain'].impressions).toBe(2);
    expect(mockedInvoke).toHaveBeenLastCalledWith('update_ui_notice_state', {
      updates: { 'mcp-runtime-keychain': { impressionsDelta: 1, helpClicksDelta: 0 } },
    });
  });

  it('does not regress counts when the older of two write responses arrives last', async () => {
    let finishFirst!: (value: unknown) => void;
    mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => { finishFirst = resolve; }));
    const store = useUiNoticeStore.getState();
    store.recordImpression('mcp-runtime-keychain');
    const first = store.flushPending();
    store.recordImpression('mcp-runtime-keychain');
    mockedInvoke.mockResolvedValueOnce({ entries: { 'mcp-runtime-keychain': {
      firstSeenAt: 1, dismissedAt: null, impressions: 2, helpClicks: 0,
    } } });
    await store.flushPending();
    finishFirst({ entries: { 'mcp-runtime-keychain': {
      firstSeenAt: 1, dismissedAt: null, impressions: 1, helpClicks: 0,
    } } });
    await first;
    expect(useUiNoticeStore.getState().entries['mcp-runtime-keychain'].impressions).toBe(2);
  });

  it('retains visible counts when a write fails and retries without incrementing them again', async () => {
    const store = useUiNoticeStore.getState();
    store.recordImpression('mcp-runtime-keychain');
    mockedInvoke.mockRejectedValueOnce(new Error('disk busy'));
    await store.flushPending();
    expect(useUiNoticeStore.getState().entries['mcp-runtime-keychain'].impressions).toBe(1);
    mockedInvoke.mockResolvedValueOnce({ entries: { 'mcp-runtime-keychain': {
      firstSeenAt: 1, dismissedAt: null, impressions: 1, helpClicks: 0,
    } } });
    await store.flushPending();
    expect(useUiNoticeStore.getState().entries['mcp-runtime-keychain'].impressions).toBe(1);
  });
});
