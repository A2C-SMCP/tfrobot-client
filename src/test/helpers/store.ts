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
  ];

  for (const store of stores) {
    const state = store.getState();
    if ('reset' in state && typeof state.reset === 'function') {
      state.reset();
    }
  }
}
