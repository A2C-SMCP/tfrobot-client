import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

/**
 * Every dismissible notice has a stable id. The backend validates the id shape (lowercase,
 * digits, dashes, bounded length) before persisting it; this union keeps call sites honest.
 * `mcp-runtime-keychain` and `password-variable-keychain` are wired up in P2 (#104).
 */
export const NOTICE_IDS = [
  'mcp-runtime-keychain',
  'password-variable-keychain',
  'remote-control-security',
  'desktop-enumeration-unverified',
] as const;

export type NoticeId = (typeof NOTICE_IDS)[number];

export interface UiNoticeEntry {
  firstSeenAt: number | null;
  dismissedAt: number | null;
  impressions: number;
  helpClicks: number;
}

export interface UiNoticeState {
  schemaVersion: number;
  entries: Record<string, UiNoticeEntry>;
}

export interface UiNoticePatch {
  dismissed?: boolean;
  impressionsDelta?: number;
  helpClicksDelta?: number;
}

/**
 * Impressions arrive on every mount, so they are accumulated here instead of being written to
 * `settings.json` one by one. Dismissals bypass the buffer and flush immediately.
 */
const pending = new Map<NoticeId, { impressions: number; helpClicks: number }>();

/** Keeps a long session from holding impressions in memory indefinitely. */
export const IMPRESSION_FLUSH_THRESHOLD = 20;

function pendingTotal(): number {
  let total = 0;
  for (const counters of pending.values()) total += counters.impressions;
  return total;
}

function takePending(id: NoticeId): UiNoticePatch {
  const counters = pending.get(id);
  pending.delete(id);
  if (!counters) return {};
  return {
    impressionsDelta: counters.impressions,
    helpClicksDelta: counters.helpClicks,
  };
}

function takeAllPending(): Record<string, UiNoticePatch> {
  const updates: Record<string, UiNoticePatch> = {};
  for (const id of [...pending.keys()]) {
    const patch = takePending(id);
    if (patch.impressionsDelta || patch.helpClicksDelta) updates[id] = patch;
  }
  return updates;
}

/** A write that failed did not consume the counters it carried; put them back for the next flush. */
function restorePending(updates: Record<string, UiNoticePatch>) {
  for (const [id, patch] of Object.entries(updates)) {
    const counters = pending.get(id as NoticeId) ?? { impressions: 0, helpClicks: 0 };
    counters.impressions += patch.impressionsDelta ?? 0;
    counters.helpClicks += patch.helpClicksDelta ?? 0;
    pending.set(id as NoticeId, counters);
  }
}

/** Counters only increase. The UI includes buffered and in-flight events already, so an
 * older server response must neither erase them nor add them a second time. */
function mergeServerEntries(
  incoming: Record<string, UiNoticeEntry>,
  local: Record<string, UiNoticeEntry>,
): Record<string, UiNoticeEntry> {
  const merged = { ...incoming };
  for (const [id, entry] of Object.entries(local)) {
    const server = incoming[id];
    merged[id] = {
      firstSeenAt: server?.firstSeenAt ?? entry.firstSeenAt,
      dismissedAt: server?.dismissedAt ?? entry.dismissedAt,
      impressions: Math.max(server?.impressions ?? 0, entry.impressions),
      helpClicks: Math.max(server?.helpClicks ?? 0, entry.helpClicks),
    };
  }
  return merged;
}

function emptyEntry(): UiNoticeEntry {
  return { firstSeenAt: Date.now(), dismissedAt: null, impressions: 0, helpClicks: 0 };
}

interface UiNoticeStoreState {
  entries: Record<string, UiNoticeEntry>;
  /** True once the persisted state has been read (or the read failed), so callers can render. */
  loaded: boolean;
  error: string | null;
  fetch: () => Promise<void>;
  isDismissed: (id: NoticeId) => boolean;
  dismiss: (id: NoticeId) => Promise<void>;
  recordImpression: (id: NoticeId) => void;
  recordHelpClick: (id: NoticeId) => Promise<void>;
  flushPending: () => Promise<void>;
  reset: () => void;
}

const initialState = {
  entries: {} as Record<string, UiNoticeEntry>,
  loaded: false,
  error: null as string | null,
};

export const useUiNoticeStore = create<UiNoticeStoreState>((set, get) => ({
  ...initialState,

  reset: () => {
    pending.clear();
    set(initialState);
  },

  fetch: async () => {
    try {
      const state = await invoke<UiNoticeState>('get_ui_notice_state');
      set({ entries: mergeServerEntries(state?.entries ?? {}, get().entries), loaded: true, error: null });
    } catch (reason) {
      // Fail open: with no state nothing looks dismissed, so a notice is shown rather than
      // silently dropped because the read failed.
      set({ loaded: true, error: String(reason) });
    }
  },

  isDismissed: (id) => Boolean(get().entries[id]?.dismissedAt),

  dismiss: async (id) => {
    if (get().isDismissed(id)) return;
    const previous = get().entries;
    const patch = takePending(id);
    set({
      entries: {
        ...previous,
        [id]: {
          firstSeenAt: previous[id]?.firstSeenAt ?? Date.now(),
          dismissedAt: Date.now(),
          impressions: previous[id]?.impressions ?? 0,
          helpClicks: previous[id]?.helpClicks ?? 0,
        },
      },
    });
    try {
      const state = await invoke<UiNoticeState>('update_ui_notice_state', {
        updates: { [id]: { ...patch, dismissed: true } },
      });
      set({ entries: mergeServerEntries(state?.entries ?? {}, get().entries), error: null });
    } catch (reason) {
      restorePending({ [id]: patch });
      set((current) => ({
        entries: {
          ...current.entries,
          [id]: { ...current.entries[id], dismissedAt: previous[id]?.dismissedAt ?? null },
        },
        error: String(reason),
      }));
    }
  },

  recordImpression: (id) => {
    const counters = pending.get(id) ?? { impressions: 0, helpClicks: 0 };
    counters.impressions += 1;
    pending.set(id, counters);
    set((current) => {
      const entry = current.entries[id] ?? emptyEntry();
      return { entries: { ...current.entries, [id]: { ...entry, impressions: entry.impressions + 1 } } };
    });
    if (pendingTotal() >= IMPRESSION_FLUSH_THRESHOLD) void get().flushPending();
  },

  recordHelpClick: async (id) => {
    const counters = pending.get(id) ?? { impressions: 0, helpClicks: 0 };
    counters.helpClicks += 1;
    pending.set(id, counters);
    set((current) => {
      const entry = current.entries[id] ?? emptyEntry();
      return { entries: { ...current.entries, [id]: { ...entry, helpClicks: entry.helpClicks + 1 } } };
    });
    await get().flushPending();
  },

  flushPending: async () => {
    const updates = takeAllPending();
    if (Object.keys(updates).length === 0) return;
    try {
      const state = await invoke<UiNoticeState>('update_ui_notice_state', { updates });
      set({ entries: mergeServerEntries(state?.entries ?? {}, get().entries), error: null });
    } catch (reason) {
      restorePending(updates);
      set({ error: String(reason) });
    }
  },
}));

/**
 * Best-effort flush for batched impressions when the window is hidden or torn down.
 *
 * State changes (dismiss, help click, threshold) already write immediately; these DOM events only
 * cover session-level impression counts, which is why wiring them here is enough and the tray /
 * close-request lifecycle in Rust does not need to know about notices.
 */
export function installUiNoticeFlushHooks(): () => void {
  const flush = () => {
    void useUiNoticeStore.getState().flushPending();
  };
  const flushIfHidden = () => {
    if (document.visibilityState === 'hidden') flush();
  };
  window.addEventListener('pagehide', flush);
  document.addEventListener('visibilitychange', flushIfHidden);
  return () => {
    window.removeEventListener('pagehide', flush);
    document.removeEventListener('visibilitychange', flushIfHidden);
  };
}
