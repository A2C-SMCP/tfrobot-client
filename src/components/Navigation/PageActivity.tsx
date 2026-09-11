import type { PropsWithChildren } from 'react';
import { PageActivityContext, usePageActive } from './pageActivityState';

export function PageActivity({ active, children }: PropsWithChildren<{ active: boolean }>) {
  const parent = usePageActive();
  return <PageActivityContext.Provider value={parent && active}>{children}</PageActivityContext.Provider>;
}
