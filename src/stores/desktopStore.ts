import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { formatRuntimeActionError } from '@/utils/runtimeActionError';
import type { ComputerRuntimeSnapshot } from '@/stores/runtimeSnapshot';

export interface DesktopWindow {
  bundleId: string;
  uri: string;
  title: string;
  server: string;
  description?: string;
  mime_type?: string;
}

export interface WindowContent {
  type: 'text' | 'blob';
  uri: string;
  mime_type?: string;
  text?: string;
  blob?: string;
}

export interface WindowDetail {
  bundleId: string;
  uri: string;
  title?: string;
  server: string;
  contents: WindowContent[];
}

export type DesktopEnumerationStatus =
  | 'complete'
  | 'partial'
  | 'unavailable'
  | 'failed'
  | 'unverified';

export interface DesktopEnumerationResult {
  status: DesktopEnumerationStatus;
  windows: DesktopWindow[];
}

export interface DesktopInstanceState {
  runtimeKey: string | null;
  windows: DesktopWindow[];
  enumerationStatus: DesktopEnumerationStatus | null;
  loaded: boolean;
  loading: boolean;
  error: string | null;
  listRequestId: number;
  windowDetails: Record<string, WindowDetail>;
  loadingDetails: Record<string, boolean>;
  detailErrors: Record<string, string>;
  detailRequestIds: Record<string, number>;
}

interface DesktopState {
  instances: Record<string, DesktopInstanceState>;
  bindRuntime: (instanceId: string, runtimeKey: string) => void;
  fetchDesktop: (instanceId: string, runtimeKey: string, uri?: string) => Promise<void>;
  fetchWindowDetail: (
    instanceId: string,
    runtimeKey: string,
    bundleId: string,
    uri: string,
  ) => Promise<void>;
  reset: () => void;
}

const emptyInstanceState: DesktopInstanceState = {
  runtimeKey: null,
  windows: [],
  enumerationStatus: null,
  loaded: false,
  loading: false,
  error: null,
  listRequestId: 0,
  windowDetails: {},
  loadingDetails: {},
  detailErrors: {},
  detailRequestIds: {},
};

const initialState = {
  instances: {} as Record<string, DesktopInstanceState>,
};

let nextRequestToken = 1;

function requestToken(): number {
  const token = nextRequestToken;
  nextRequestToken += 1;
  return token;
}

function emptyStateForRuntime(runtimeKey: string): DesktopInstanceState {
  return {
    ...emptyInstanceState,
    runtimeKey,
  };
}

function instanceState(
  instances: Record<string, DesktopInstanceState>,
  instanceId: string,
): DesktopInstanceState {
  return instances[instanceId] ?? emptyInstanceState;
}

function updateInstance(
  instances: Record<string, DesktopInstanceState>,
  instanceId: string,
  update: (current: DesktopInstanceState) => DesktopInstanceState,
): Record<string, DesktopInstanceState> {
  return {
    ...instances,
    [instanceId]: update(instanceState(instances, instanceId)),
  };
}

export function desktopWindowKey(
  window: Pick<DesktopWindow, 'bundleId' | 'uri'>,
): string {
  return JSON.stringify([window.bundleId, window.uri]);
}

export function desktopRuntimeKey(
  runtime: Pick<
    ComputerRuntimeSnapshot,
    'incarnation' | 'generation' | 'capability_revision' | 'mcp_servers' | 'active_mcp_servers'
  >,
): string {
  return [
    runtime.incarnation,
    runtime.generation,
    runtime.capability_revision,
    runtime.mcp_servers,
    runtime.active_mcp_servers,
  ].join(':');
}

export function selectDesktopInstance(
  state: DesktopState,
  instanceId: string,
  runtimeKey?: string,
): DesktopInstanceState {
  const current = instanceState(state.instances, instanceId);
  return runtimeKey === undefined || current.runtimeKey === runtimeKey
    ? current
    : emptyInstanceState;
}

export const useDesktopStore = create<DesktopState>((set) => ({
  ...initialState,

  reset: () => set(initialState),

  bindRuntime: (instanceId, runtimeKey) => {
    set((state) => {
      const current = instanceState(state.instances, instanceId);
      if (current.runtimeKey === runtimeKey) return state;
      return {
        instances: {
          ...state.instances,
          [instanceId]: emptyStateForRuntime(runtimeKey),
        },
      };
    });
  },

  fetchDesktop: async (instanceId: string, runtimeKey: string, uri?: string) => {
    const requestId = requestToken();
    set((state) => ({
      instances: updateInstance(state.instances, instanceId, (current) => ({
        ...(current.runtimeKey === runtimeKey ? current : emptyStateForRuntime(runtimeKey)),
        loading: true,
        error: null,
        listRequestId: requestId,
        windowDetails: {},
        loadingDetails: {},
        detailErrors: {},
        detailRequestIds: {},
      })),
    }));

    try {
      const result = await invoke<DesktopEnumerationResult>('get_desktop', {
        instanceId,
        uri: uri ?? null,
      });
      set((state) => {
        const current = instanceState(state.instances, instanceId);
        if (current.runtimeKey !== runtimeKey || current.listRequestId !== requestId) return state;
        return {
          instances: updateInstance(state.instances, instanceId, (latest) => ({
            ...latest,
            windows: result.windows,
            enumerationStatus: result.status,
            loaded: true,
            loading: false,
            error: null,
          })),
        };
      });
    } catch (error) {
      set((state) => {
        const current = instanceState(state.instances, instanceId);
        if (current.runtimeKey !== runtimeKey || current.listRequestId !== requestId) return state;
        return {
          instances: updateInstance(state.instances, instanceId, (latest) => ({
            ...latest,
            loading: false,
            error: formatRuntimeActionError(error),
          })),
        };
      });
    }
  },

  fetchWindowDetail: async (
    instanceId: string,
    runtimeKey: string,
    bundleId: string,
    uri: string,
  ) => {
    const key = desktopWindowKey({ bundleId, uri });
    const requestId = requestToken();
    set((state) => ({
      instances: updateInstance(state.instances, instanceId, (latest) => {
        const current = latest.runtimeKey === runtimeKey
          ? latest
          : emptyStateForRuntime(runtimeKey);
        return {
          ...current,
          loadingDetails: { ...current.loadingDetails, [key]: true },
          detailErrors: Object.fromEntries(
            Object.entries(current.detailErrors).filter(([entryKey]) => entryKey !== key),
          ),
          detailRequestIds: { ...current.detailRequestIds, [key]: requestId },
        };
      }),
    }));

    try {
      const detail = await invoke<WindowDetail>('get_window_detail', {
        instanceId,
        bundleId,
        uri,
      });
      set((state) => {
        const latest = instanceState(state.instances, instanceId);
        if (
          latest.runtimeKey !== runtimeKey
          || latest.detailRequestIds[key] !== requestId
        ) return state;
        const { [key]: _loading, ...loadingDetails } = latest.loadingDetails;
        return {
          instances: updateInstance(state.instances, instanceId, (entry) => ({
            ...entry,
            windowDetails: { ...entry.windowDetails, [key]: detail },
            loadingDetails,
          })),
        };
      });
    } catch (error) {
      set((state) => {
        const latest = instanceState(state.instances, instanceId);
        if (
          latest.runtimeKey !== runtimeKey
          || latest.detailRequestIds[key] !== requestId
        ) return state;
        const { [key]: _loading, ...loadingDetails } = latest.loadingDetails;
        return {
          instances: updateInstance(state.instances, instanceId, (entry) => ({
            ...entry,
            loadingDetails,
            detailErrors: {
              ...entry.detailErrors,
              [key]: formatRuntimeActionError(error),
            },
          })),
        };
      });
    }
  },
}));
