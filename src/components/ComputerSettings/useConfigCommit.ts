import { useCallback, useContext, useMemo, useRef, useSyncExternalStore } from 'react';
import { ScopeContext } from '@/components/Navigation/navigationMemoryState';

export type ConfigSaveState = 'saved' | 'dirty' | 'saving' | 'error';
interface CommitRecord<T, R> {
  state: ConfigSaveState;
  revision: number;
  last?: T;
  queue: Promise<unknown>;
  save: (value: T) => Promise<R>;
  error?: unknown;
  listeners: Set<() => void>;
}
function publish<T, R>(record: CommitRecord<T, R>, state: ConfigSaveState) {
  record.state = state;
  for (const notify of [...record.listeners]) notify();
}

/** Submission ownership follows the Computer's in-memory edit scope, including unmounted pages. */
export function useConfigCommit<T, R = unknown>(identity: string, save: (value: T) => Promise<R>, initialState: ConfigSaveState = 'saved') {
  const scope = useContext(ScopeContext);
  const local = useRef(new Map<string, unknown>());
  const records = scope ?? local.current;
  const key = `configCommit:${identity}`;
  const existing = records.get(key) as CommitRecord<T, R> | undefined;
  const record = useMemo<CommitRecord<T, R>>(() => existing ?? { state: initialState, revision: 0, queue: Promise.resolve(), save, listeners: new Set() }, [existing, initialState, save]);
  if (!existing) records.set(key, record);
  record.save = save;
  const subscribe = useCallback((notify: () => void) => {
    record.listeners.add(notify);
    return () => { record.listeners.delete(notify); };
  }, [record]);
  const state = useSyncExternalStore(subscribe, () => record.state);
  const dirty = useCallback(() => { record.revision += 1; record.error = undefined; publish(record, 'dirty'); }, [record]);
  const capture = useCallback(() => {
    const revision = record.revision;
    const state = record.state;
    return () => records.get(key) === record && record.revision === revision && record.state === state;
  }, [record, records, key]);
  const commit = useCallback(async (value: T, onSaved?: (result: R) => void, onPersisted?: (result: R) => void): Promise<boolean> => {
    const revision = ++record.revision;
    record.last = value;
    record.error = undefined;
    publish(record, 'saving');
    const operation = record.save;
    const next = record.queue.catch(() => undefined).then(() => {
      if (records.get(key) !== record) throw new Error('Configuration editor was discarded');
      return operation(value);
    });
    record.queue = next;
    try {
      const result = await next;
      if (result === false) throw new Error('Configuration was not saved');
      if (records.get(key) !== record) return false;
      onPersisted?.(result);
      const acknowledged = record.revision === revision;
      if (acknowledged) { onSaved?.(result); publish(record, 'saved'); }
      return acknowledged;
    } catch (error) {
      if (records.get(key) === record && record.revision === revision) { record.error = error; publish(record, 'error'); }
      return false;
    }
  }, [record, records, key]);
  const reset = useCallback(() => { record.revision += 1; record.error = undefined; record.last = undefined; publish(record, 'saved'); }, [record]);
  const getError = useCallback(() => record.error, [record]);
  return { state, dirty, capture, commit, reset, last: record.last, getError, retry: () => record.last === undefined ? Promise.resolve(false) : commit(record.last) };
}
