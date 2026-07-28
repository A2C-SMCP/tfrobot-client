import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import type { LogEntry } from './logStore';
import { useComputerStore, type ConnectionStateSummary } from './computerStore';
import {
  getClientConnectionAuthority,
  legacyClientConnectionState,
  setClientConnectionAuthority,
  type ClientConnectionAuthority,
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
  connection_state?: ClientConnectionAuthority;
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
  connectionState = data.connection_state ?? legacyClientConnectionState(
    data.client_connection_present ?? (data.connection_context ? true : data.connected),
    data.connection_context,
    data.connection_revision ?? 0,
    runtime,
  ),
): ComputerOverviewData {
  const projection = projectRuntimeSnapshot(runtime, connectionState.status === 'connected');
  return {
    ...data,
    runtime,
    running: projection.running,
    connection_state: connectionState,
    connected: connectionState.status === 'connected',
    client_connection_present: connectionState.present,
    connection_context: connectionState.context,
    connection_revision: connectionState.revision,
    connection_url: connectionState.present
      ? connectionState.context?.url ?? data.connection_url
      : undefined,
    connection_profile: connectionState.present
      ? connectionState.context?.profile_name ?? data.connection_profile
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
      const connectionState = data.connection_state ?? legacyClientConnectionState(
        data.client_connection_present ?? (connectionContext ? true : data.connected),
        connectionContext,
        data.connection_revision ?? 0,
        data.runtime,
      );
      setClientConnectionAuthority(instanceId, connectionState, data.runtime);
      useRuntimeStore.getState().receiveSnapshot(instanceId, data.runtime, connectionState);
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
          authority
            ?? (data.runtime.incarnation === resolvedRuntime.incarnation
              ? data.connection_state
              : undefined)
            ?? legacyClientConnectionState(false, null, 0, resolvedRuntime),
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
        authority
          ?? (state.data.runtime.incarnation === resolvedRuntime.incarnation
            ? state.data.connection_state
            : undefined)
          ?? legacyClientConnectionState(false, null, 0, resolvedRuntime),
      ),
    };
  }),
}));
