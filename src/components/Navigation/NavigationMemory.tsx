import { useContext, type PropsWithChildren } from 'react';
import type { NavigationMemory } from '@/stores/navigationStore';
import { MemoryContext, ScopeContext } from './navigationMemoryState';

export function NavigationMemoryProvider({ memory, children }: PropsWithChildren<{ memory: NavigationMemory }>) {
  return <MemoryContext.Provider value={memory}>{children}</MemoryContext.Provider>;
}

export function NavigationScope({ id, children }: PropsWithChildren<{ id: string }>) {
  const memory = useContext(MemoryContext);
  return <ScopeContext.Provider value={memory?.scope(id) ?? null}>{children}</ScopeContext.Provider>;
}
