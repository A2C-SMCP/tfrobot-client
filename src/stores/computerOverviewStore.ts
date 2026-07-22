import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import type { LogEntry } from './logStore';
import { useComputerStore, type ConnectionStateSummary } from './computerStore';
import {
  getClientConnectionAuthority,
  setClientConnectionAuthority,
} from './connectionAuthority';
import {
  isRuntimeSnapshotInstanceDeleted,
  projectRuntimeSnapshot,
  resolveRuntimeSnapshot,
  type ComputerRuntimeSnapshot,
} from './runtimeSnapshot';

export interface ComputerOverviewData {
  id: string;
  name: string;
  running: boolean;
  runtime: ComputerRuntimeSnapshot;
  connected: boolean;
  client_connection_present?: boolean;
  connection_revision?: number;
  connection_context?: ConnectionStateSummary | null;
  connection_url?: string;
  connection_profile?: string;
  robot_name?: string;
  mcp_total: number;
  mcp_running: number;
  mcp_stopped: number;
  tools_count: number;
  recent_logs: LogEntry[];
}

interface ComputerOverviewState {
  data: ComputerOverviewData | null;
  loading: boolean;
  error: string | null;
  activeInstanceId: string | null;
  requestId: number;
  fetchOverview: (instanceId: string) => Promise<void>;
  applyRuntimeSnapshot: (instanceId: string, runtime: ComputerRuntimeSnapshot) => void;
  reset: () => void;
}

const initialState = {
  data: null as ComputerOverviewData | null,
  loading: false,
  error: null as string | null,
  activeInstanceId: null as string | null,
  requestId: 0,
};

function projectRuntime(
  data: ComputerOverviewData,
  runtime: ComputerRuntimeSnapshot,
  clientConnectionPresent = data.client_connection_present
    ?? (data.connection_context ? true : data.connected),
  connectionContext = data.connection_context,
  connectionRevision = data.connection_revision,
): ComputerOverviewData {
  const projection = projectRuntimeSnapshot(runtime, clientConnectionPresent);
  return {
    ...data,
    runtime,
    running: projection.running,
    connected: projection.businessConnected,
    client_connection_present: clientConnectionPresent,
    connection_context: clientConnectionPresent ? connectionContext ?? null : null,
    connection_revision: connectionRevision,
    connection_url: projection.businessConnected
      ? connectionContext?.url ?? data.connection_url
      : undefined,
    connection_profile: projection.businessConnected
      ? connectionContext?.profile_name ?? data.connection_profile
      : undefined,
    mcp_total: projection.mcpServerCount,
    mcp_running: projection.activeMcpServerCount,
    mcp_stopped: projection.stoppedMcpServerCount,
    tools_count: projection.toolCount,
  };
}

export const useComputerOverviewStore = create<ComputerOverviewState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchOverview: async (instanceId: string) => {
    const requestId = get().requestId + 1;
    set({ activeInstanceId: instanceId, requestId, loading: true, error: null });
    try {
      const data = await invoke<ComputerOverviewData>('get_computer_overview_data', { instanceId });
      if (get().requestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      if (isRuntimeSnapshotInstanceDeleted(instanceId)) {
        set({ data: null, loading: false });
        return;
      }
      const { useRuntimeStore } = await import('./runtimeStore');
      const connectionContext = data.connection_context ?? null;
      setClientConnectionAuthority(
        instanceId,
        data.client_connection_present ?? (connectionContext ? true : data.connected),
        connectionContext,
        data.connection_revision ?? 0,
        data.runtime,
      );
      useRuntimeStore.getState().receiveSnapshot(instanceId, data.runtime);
      if (get().requestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      const instance = useComputerStore.getState().instances.find((item) => item.id === instanceId);
      const resolvedRuntime = resolveRuntimeSnapshot(
        instanceId,
        useComputerOverviewStore.getState().data?.id === instanceId
          ? useComputerOverviewStore.getState().data?.runtime
          : undefined,
        data.runtime,
      );
      const authority = getClientConnectionAuthority(instanceId, resolvedRuntime.incarnation);
      const projectedData = instance ? {
        ...data,
        name: instance.name,
        robot_name: instance.robotName,
        connection_profile: instance.connectionProfile,
        connection_url: instance.connectionUrl,
      } : data;
      set(() => ({
        data: projectRuntime(
          projectedData,
          resolvedRuntime,
          authority?.present
            ?? (data.runtime.incarnation === resolvedRuntime.incarnation
              ? data.client_connection_present ?? data.connected
              : false),
          authority?.context
            ?? (data.runtime.incarnation === resolvedRuntime.incarnation
              ? data.connection_context
              : null),
          authority?.revision ?? data.connection_revision,
        ),
        loading: false,
      }));
    } catch (e) {
      if (get().requestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: String(e), loading: false });
    }
  },

  applyRuntimeSnapshot: (instanceId, runtime) => set((state) => {
    if (!state.data || state.data.id !== instanceId) return state;
    const instance = useComputerStore.getState().instances.find((item) => item.id === instanceId);
    const resolvedRuntime = resolveRuntimeSnapshot(instanceId, state.data.runtime, runtime, true);
    const authority = getClientConnectionAuthority(instanceId, resolvedRuntime.incarnation);
    return {
      data: projectRuntime(
        instance ? {
          ...state.data,
          name: instance.name,
          robot_name: instance.robotName,
          connection_profile: instance.connectionProfile,
          connection_url: instance.connectionUrl,
        } : state.data,
        resolvedRuntime,
        authority?.present
          ?? (state.data.runtime.incarnation === resolvedRuntime.incarnation
            ? state.data.client_connection_present ?? state.data.connected
            : false),
        authority?.context
          ?? (state.data.runtime.incarnation === resolvedRuntime.incarnation
            ? state.data.connection_context
            : null),
        authority?.revision ?? state.data.connection_revision,
      ),
    };
  }),
}));
