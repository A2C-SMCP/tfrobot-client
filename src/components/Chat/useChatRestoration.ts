import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { ConversationWorkspaceBinding } from '@turingfocus/chat-kit';

let lastWriteRevision = 0;
function nextWriteRevision() {
  lastWriteRevision = Math.max(lastWriteRevision + 1, Date.now() * 1000);
  return lastWriteRevision;
}

/** Persist committed selections, never pending clicks or superseded load results. */
export function useChatRestoration(workspace: ConversationWorkspaceBinding, leaseId?: string) {
  const controller = workspace.controller;
  const epoch = useRef(0);
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [empty, setEmpty] = useState(false);
  const retrySave = useRef<(() => void) | undefined>(undefined);

  useEffect(() => {
    if (!controller || !leaseId) return;
    let active = true;
    let lastId: string | undefined;
    let revision = 0;
    const subscription = controller.subscribe((snapshot) => {
      if (snapshot.selectionStatus !== 'ready' || !snapshot.selectedConversationId
        || snapshot.selectedConversationId === lastId) return;
      lastId = snapshot.selectedConversationId;
      const conversationId = lastId;
      const writeRevision = nextWriteRevision();
      revision = writeRevision;
      const save = () => {
        void invoke('chat_remember_conversation', { leaseId, conversationId, revision: writeRevision })
          .then(() => {
            if (active && revision === writeRevision) { retrySave.current = undefined; setError(false); }
          })
          .catch(() => {
            if (active && revision === writeRevision) { retrySave.current = save; setError(true); }
          });
      };
      retrySave.current = undefined;
      save();
    });
    return () => { active = false; retrySave.current = undefined; subscription.dispose(); };
  }, [controller, leaseId]);

  useEffect(() => {
    if (!controller || !leaseId) return;
    const operation = ++epoch.current;
    let active = true;
    const current = () => active && epoch.current === operation && !controller.disposed;
    setError(false);
    setEmpty(false);
    setRestoring(true);
    const restore = async () => {
      const id = await invoke<string | null>('chat_get_recent_conversation', { leaseId });
      if (!current()) return;
      const unavailable = new Set<string>();
      if (id) {
        const result = await controller.selectConversation(id);
        if (!current()) return;
        if (result.ok) return;
        if (result.error.code !== 'not-found' && result.error.code !== 'authorization') throw result.error;
        unavailable.add(id);
      }
      const list = await controller.refresh();
      if (!current()) return;
      if (!list.ok) throw list.error;
      // Traverse server pagination only after an explicit unavailable response; no timed probing.
      for (;;) {
        const snapshot = controller.getSnapshot();
        for (const conversation of snapshot.conversations) {
          if (unavailable.has(conversation.id)) continue;
          const result = await controller.selectConversation(conversation.id);
          if (!current()) return;
          if (result.ok) return;
          if (result.error.code !== 'not-found' && result.error.code !== 'authorization') throw result.error;
          unavailable.add(conversation.id);
        }
        if (!snapshot.nextCursor) { setEmpty(true); return; }
        const more = await controller.loadMore();
        if (!current()) return;
        if (!more?.ok) throw new Error('Conversation list could not be loaded');
      }
    };
    void restore().catch(() => { if (current()) setError(true); })
      .finally(() => { if (current()) setRestoring(false); });
    return () => { active = false; };
  }, [attempt, controller, leaseId]);

  const manualSelection = useCallback(() => {
    ++epoch.current;
    setRestoring(false);
    setEmpty(false);
    setError(false);
  }, []);
  const retry = useCallback(() => {
    if (retrySave.current) retrySave.current();
    else setAttempt((value) => value + 1);
  }, []);
  return { error, restoring, empty, retry, manualSelection };
}
