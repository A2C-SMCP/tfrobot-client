import type { ComputerRuntimeSnapshot } from '@/stores/runtimeSnapshot';

export const DESKTOP_OPERATIONAL_LIFECYCLES = new Set<
  ComputerRuntimeSnapshot['lifecycle']
>([
  'started',
  'connected',
  'joined_office',
  'degraded',
]);

export function canEnumerateDesktopResources(runtime: ComputerRuntimeSnapshot): boolean {
  return DESKTOP_OPERATIONAL_LIFECYCLES.has(runtime.lifecycle)
    && runtime.active_mcp_servers > 0;
}
