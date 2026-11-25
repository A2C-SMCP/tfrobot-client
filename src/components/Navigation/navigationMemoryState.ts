import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState, useSyncExternalStore, type Dispatch, type SetStateAction } from 'react';
import type { FormInstance } from 'antd';
import type { NavigationMemory } from '@/stores/navigationStore';
export const MemoryContext = createContext<NavigationMemory | null>(null);
export const ScopeContext = createContext<Map<string, unknown> | null>(null);
const subscriptions = new WeakMap<Map<string, unknown>, Map<string, { revision: number; listeners: Set<() => void> }>>();
function subscription(scope: Map<string, unknown>, key: string) {
  let keys = subscriptions.get(scope);
  if (!keys) { keys = new Map(); subscriptions.set(scope, keys); }
  let entry = keys.get(key);
  if (!entry) { entry = { revision: 0, listeners: new Set() }; keys.set(key, entry); }
  return entry;
}
function notify(scope: Map<string, unknown>, key: string) {
  const entry = subscription(scope, key);
  entry.revision += 1;
  for (const listener of [...entry.listeners]) listener();
}

/** Lightweight view state survives an object's component tree being released. */
export function useNavigationState<T>(key: string, initial: T | (() => T)): [T, Dispatch<SetStateAction<T>>] {
  const scope = useContext(ScopeContext);
  const changes = scope ? subscription(scope, key) : null;
  useSyncExternalStore(useCallback((listener: () => void) => {
    changes?.listeners.add(listener);
    return () => { changes?.listeners.delete(listener); };
  }, [changes]), () => changes?.revision ?? 0);
  const read = () => scope?.has(key)
    ? scope.get(key) as T
    : typeof initial === 'function' ? (initial as () => T)() : initial;
  const [snapshot, setSnapshot] = useState(() => ({ scope, key, value: read() }));
  let current = snapshot;
  if (snapshot.scope !== scope || snapshot.key !== key
    || (scope?.has(key) && !Object.is(snapshot.value, scope.get(key)))) {
    current = { scope, key, value: read() };
    setSnapshot(current);
  }
  const currentRef = useRef(current);
  currentRef.current = current;
  const update = useCallback<Dispatch<SetStateAction<T>>>((next) => {
    const previous = currentRef.current;
    const ownsView = previous.scope === scope && previous.key === key;
    if (!ownsView && !scope?.has(key)) return;
    const value = scope?.has(key) ? scope.get(key) as T : previous.value;
    const result = typeof next === 'function' ? (next as (value: T) => T)(value) : next;
    if (Object.is(value, result)) return;
    const snapshot = { scope, key, value: result };
    if (scope) { scope.set(key, result); notify(scope, key); }
    if (ownsView) { currentRef.current = snapshot; setSnapshot(snapshot); }
  }, [key, scope]);
  return [current.value, update];
}

// Ant Form reports nested changes as partial objects, but array edits replace
// the field. Preserve sibling dirty fields and explicit undefined (clear).
function mergeFields<T extends object>(baseline: T, changes: Partial<T>): T {
  const merged = { ...baseline };
  for (const key of Object.keys(changes) as (keyof T)[]) {
    const value = changes[key];
    const previous = baseline[key];
    merged[key] = (value && previous && typeof value === 'object' && typeof previous === 'object'
      && !Array.isArray(value) && !Array.isArray(previous)
      ? mergeFields(previous, value) : value) as T[keyof T];
  }
  return merged;
}

/** Merge refreshed fields around dirty values; callers clear the draft after explicit save. */
export function useNavigationForm<T extends object>(key: string, form: FormInstance<T>, initial: T) {
  const [draft, setDraft] = useNavigationState<Partial<T>>(key, {});
  const scope = useContext(ScopeContext);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const baselineRef = useRef(initial);
  baselineRef.current = initial;
  const signature = JSON.stringify(initial);
  // Hydrate on restore or baseline refresh, never in response to typing. Ant Form
  // also permits programmatic updates during an input event.
  useLayoutEffect(() => {
    form.setFieldsValue(mergeFields(baselineRef.current, draftRef.current) as Parameters<FormInstance<T>['setFieldsValue']>[0]);
  }, [form, key, scope, signature]);
  return {
    dirtyFields: Object.keys(draft) as (keyof T)[],
    onValuesChange: (changed: Partial<T>) => setDraft((current) => mergeFields(current, changed)),
    setValues: (changed: Partial<T>) => {
      form.setFieldsValue(mergeFields(form.getFieldsValue(true), changed));
      setDraft((current) => mergeFields(current, changed));
    },
    clearFields: (keys: (keyof T)[]) => setDraft((current) => {
      const next = { ...current };
      for (const field of keys) delete next[field];
      return next;
    }),
    clearDraft: () => setDraft({}),
  };
}

/** Explicit save/discard and object deletion remove no-longer-needed draft entries. */
export function useForgetNavigationState() {
  const scope = useContext(ScopeContext);
  return (prefix: string) => {
    if (!scope) return;
    for (const key of [...scope.keys()]) if (key.startsWith(prefix) || key.startsWith(`configCommit:${prefix}`)) {
      scope.delete(key); notify(scope, key);
    }
  };
}
