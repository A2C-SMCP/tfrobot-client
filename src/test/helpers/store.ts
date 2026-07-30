import { useMcpStore } from '@/stores/mcpStore';
import { useConnectionStore } from '@/stores/connectionStore';
import { useDashboardStore } from '@/stores/dashboardStore';
import { useDebugStore } from '@/stores/debugStore';
import { useInputStore } from '@/stores/inputStore';
import { useLogStore } from '@/stores/logStore';
import { useSettingsStore } from '@/stores/settingsStore';
import { useThemeStore } from '@/stores/themeStore';
import { useDesktopStore } from '@/stores/desktopStore';
import { useManagerStore } from '@/stores/managerStore';
import {
  useComputerStore,
  type ComputerRuntimeSnapshot,
} from '@/stores/computerStore';
import { useSkillStore } from '@/stores/skillStore';
import { useRuntimeStore } from '@/stores/runtimeStore';
import { useSdkConfigStore } from '@/stores/sdkConfigStore';

function runtimeActionsForTest(
  lifecycle: ComputerRuntimeSnapshot['lifecycle'],
): ComputerRuntimeSnapshot['actions'] {
  const inactive = ['created', 'stopped', 'shutdown', 'error'].includes(lifecycle);
  const locallyOperational = ['started', 'connected', 'joined_office', 'degraded']
    .includes(lifecycle);
  return {
    can_start: inactive,
    can_stop: locallyOperational,
    can_restart: locallyOperational,
    can_reload: inactive || locallyOperational,
    can_connect: lifecycle === 'started',
    can_disconnect: ['connected', 'joined_office', 'degraded'].includes(lifecycle),
    can_manage_mcp: ['started', 'connected', 'joined_office'].includes(lifecycle),
  };
}

export function runtimeSnapshot(
  overrides: Partial<ComputerRuntimeSnapshot> = {},
): ComputerRuntimeSnapshot {
  const lifecycle = overrides.lifecycle ?? 'started';
  return {
    incarnation: 1,
    generation: 1,
    snapshot_revision: 1,
    lifecycle,
    actions: overrides.actions ?? runtimeActionsForTest(lifecycle),
    config_revision: 0,
    capability_revision: 0,
    mcp_servers: 0,
    active_mcp_servers: 0,
    tools: 0,
    skills: 0,
    last_error: null,
    degraded_reason: null,
    ...overrides,
  };
}

/**
 * Reset all Zustand stores to their initial state.
 * Call this in beforeEach to isolate tests.
 */
export function resetAllStores() {
  const stores = [
    useMcpStore,
    useConnectionStore,
    useDashboardStore,
    useDebugStore,
    useInputStore,
    useLogStore,
    useSettingsStore,
    useThemeStore,
    useDesktopStore,
    useManagerStore,
    useComputerStore,
    useSkillStore,
    useRuntimeStore,
    useSdkConfigStore,
  ];

  for (const store of stores) {
    const state = store.getState();
    if ('reset' in state && typeof state.reset === 'function') {
      state.reset();
    }
  }
}
