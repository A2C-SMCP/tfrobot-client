import { createContext, useCallback, useContext, useLayoutEffect, useRef } from 'react';
export const PageActivityContext = createContext(true);

export function usePageActive() { return useContext(PageActivityContext); }

/** Capture at the start of an action; navigation invalidates continuations permanently. */
export function usePageAction(scope?: unknown) {
  const active = usePageActive();
  const epoch = useRef(0);
  const live = useRef(active);
  useLayoutEffect(() => {
    live.current = active;
    epoch.current += 1;
    return () => { live.current = false; epoch.current += 1; };
  }, [active, scope]);
  return useCallback(() => {
    const started = epoch.current;
    return () => live.current && epoch.current === started;
  }, []);
}
