import { useCallback, useEffect } from 'react';
import { useUiNoticeStore, type NoticeId } from '@/stores/uiNoticeStore';

export interface NoticeLifecycle {
  /** False while the persisted state is still loading, and once the user has dismissed it. */
  visible: boolean;
  dismiss: () => void;
}

/**
 * Owns visibility and impression bookkeeping for a dismissible notice.
 *
 * Rendering stays with the caller so each placement can use the lightest thing that fits — a bar
 * inside a settings section, a field hint, or a tooltip next to the control that triggers the
 * explanation — while the stateful part lives in exactly one place.
 *
 * `enabled` carries the caller's own "should this be on screen right now" conditions. It has to be
 * part of the hook rather than only the render branch: an impression must never be counted for a
 * notice the user never saw, or the persisted counters stop meaning anything.
 */
export function useNoticeLifecycle(id: NoticeId, enabled = true): NoticeLifecycle {
  const loaded = useUiNoticeStore((state) => state.loaded);
  const dismissedAt = useUiNoticeStore((state) => state.entries[id]?.dismissedAt ?? null);
  const dismissNotice = useUiNoticeStore((state) => state.dismiss);
  const recordImpression = useUiNoticeStore((state) => state.recordImpression);

  // The store is loaded once by the app shell, not here: sections such as Desktop Resources
  // guarantee they issue no request before an explicit user action.
  const visible = enabled && loaded && !dismissedAt;
  useEffect(() => {
    if (visible) recordImpression(id);
  }, [visible, id, recordImpression]);

  const dismiss = useCallback(() => {
    void dismissNotice(id);
  }, [dismissNotice, id]);

  return { visible, dismiss };
}
