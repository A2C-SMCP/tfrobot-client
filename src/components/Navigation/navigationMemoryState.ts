import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react';
import type { FormInstance } from 'antd';
import type { NavigationMemory } from '@/stores/navigationStore';
export const MemoryContext = createContext<NavigationMemory | null>(null);
export const ScopeContext = createContext<Map<string, unknown> | null>(null);

/** Lightweight view state survives an object's component tree being released. */
export function useNavigationState<T>(key: string, initial: T | (() => T)): [T, Dispatch<SetStateAction<T>>] {
  const scope = useContext(ScopeContext);
  const read = () => scope?.has(key)
    ? scope.get(key) as T
    : typeof initial === 'function' ? (initial as () => T)() : initial;
  const [snapshot, setSnapshot] = useState(() => ({ scope, key, value: read() }));
  let current = snapshot;
  if (snapshot.scope !== scope || snapshot.key !== key) {
    current = { scope, key, value: read() };
    setSnapshot(current);
  }
  const currentRef = useRef(current);
  currentRef.current = current;
  const update = useCallback<Dispatch<SetStateAction<T>>>((next) => {
    const previous = currentRef.current;
    if (previous.scope !== scope || previous.key !== key) return;
    const result = typeof next === 'function' ? (next as (value: T) => T)(previous.value) : next;
    if (Object.is(previous.value, result)) return;
    const snapshot = { scope, key, value: result };
    currentRef.current = snapshot;
    scope?.set(key, result);
    setSnapshot(snapshot);
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
    onValuesChange: (changed: Partial<T>) => setDraft((current) => mergeFields(current, changed)),
    setValues: (changed: Partial<T>) => {
      form.setFieldsValue(mergeFields(form.getFieldsValue(true), changed));
      setDraft((current) => mergeFields(current, changed));
    },
    clearDraft: () => setDraft({}),
  };
}

/** Explicit save/discard and object deletion remove no-longer-needed draft entries. */
export function useForgetNavigationState() {
  const scope = useContext(ScopeContext);
  return (prefix: string) => {
    if (!scope) return;
    for (const key of scope.keys()) if (key.startsWith(prefix)) scope.delete(key);
  };
}
