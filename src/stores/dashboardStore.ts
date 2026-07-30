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

export interface RuntimeInfo {
  name: string;
  path?: string;
  available: boolean;
}

export interface DashboardComputerSummary {
  id: string;
  name: string;
  running: boolean;
  runtime: ComputerRuntimeSnapshot;
  connected: boolean;
  client_connection_present?: boolean;
  connection_revision?: number;
  connection_context?: ConnectionStateSummary | null;
  mcp_server_count: number;
  robot_name?: string;
  connection_profile?: string;
}

export interface DashboardData {
  computer_total: number;
  computer_running: number;
  computer_stopped: number;
  computer_connected: number;
  computers: DashboardComputerSummary[];
  recent_logs: LogEntry[];
  runtimes: RuntimeInfo[];
}

interface DashboardState {
  data: DashboardData | null;
  loading: boolean;
  error: string | null;
  requestId: number;

  fetchDashboard: () => Promise<void>;
  applyRuntimeSnapshot: (instanceId: string, runtime: ComputerRuntimeSnapshot) => void;
  reset: () => void;
}

const initialState = {
  data: null as DashboardData | null,
  loading: false,
  error: null as string | null,
  requestId: 0,
};

function projectRuntime(
  computer: DashboardComputerSummary,
  runtime: ComputerRuntimeSnapshot,
  clientConnectionPresent = computer.client_connection_present
    ?? (computer.connection_context ? true : computer.connected),
  connectionContext = computer.connection_context,
  connectionRevision = computer.connection_revision,
): DashboardComputerSummary {
  const projection = projectRuntimeSnapshot(runtime, clientConnectionPresent);
  return {
    ...computer,
    runtime,
    running: projection.running,
    connected: projection.businessConnected,
    client_connection_present: clientConnectionPresent,
    connection_context: clientConnectionPresent ? connectionContext ?? null : null,
    connection_revision: connectionRevision,
    connection_profile: projection.businessConnected
      ? connectionContext?.profile_name ?? computer.connection_profile
      : undefined,
    mcp_server_count: projection.mcpServerCount,
  };
}

function withRuntimeCounts(data: DashboardData): DashboardData {
  return {
    ...data,
    computer_total: data.computers.length,
    computer_running: data.computers.filter((computer) => computer.running).length,
    computer_stopped: data.computers.filter((computer) => !computer.running).length,
    computer_connected: data.computers.filter((computer) => computer.connected).length,
  };
}

export const useDashboardStore = create<DashboardState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchDashboard: async () => {
    const requestId = get().requestId + 1;
    set({ loading: true, error: null, requestId });
    try {
      const data = await invoke<DashboardData>('get_dashboard_data');
      if (get().requestId !== requestId) return;
      const { useRuntimeStore } = await import('./runtimeStore');
      for (const computer of data.computers) {
        const connectionContext = computer.connection_context ?? null;
        setClientConnectionAuthority(
          computer.id,
          computer.client_connection_present ?? (connectionContext ? true : computer.connected),
          connectionContext,
          computer.connection_revision ?? 0,
          computer.runtime,
        );
        useRuntimeStore.getState().receiveSnapshot(computer.id, computer.runtime);
      }
      set((state) => {
        if (state.requestId !== requestId) return {};
        const computers = data.computers
          .filter((computer) => !isRuntimeSnapshotInstanceDeleted(computer.id))
          .map((computer) => {
          const instance = useComputerStore.getState().instances.find((item) => item.id === computer.id);
          const projectedComputer = instance ? {
            ...computer,
            name: instance.name,
            robot_name: instance.robotName,
            connection_profile: instance.connectionProfile,
          } : computer;
          const current = state.data?.computers.find((item) => item.id === computer.id);
          const resolvedRuntime = resolveRuntimeSnapshot(
            computer.id,
            current?.runtime,
            computer.runtime,
          );
          const authority = getClientConnectionAuthority(computer.id, resolvedRuntime.incarnation);
          return projectRuntime(
            projectedComputer,
            resolvedRuntime,
            authority?.present
              ?? (computer.runtime.incarnation === resolvedRuntime.incarnation
                ? computer.client_connection_present ?? computer.connected
                : false),
            authority?.context
              ?? (computer.runtime.incarnation === resolvedRuntime.incarnation
                ? computer.connection_context
                : null),
            authority?.revision ?? computer.connection_revision,
          );
          });
        return { data: withRuntimeCounts({ ...data, computers }), loading: false };
      });
    } catch (e) {
      if (get().requestId === requestId) set({ error: String(e), loading: false });
    }
  },

  applyRuntimeSnapshot: (instanceId, runtime) => set((state) => {
    if (!state.data) return state;
    const computers = state.data.computers.map((computer) => {
      if (computer.id !== instanceId) return computer;
      const instance = useComputerStore.getState().instances
        .find((candidate) => candidate.id === instanceId);
      const resolvedRuntime = resolveRuntimeSnapshot(instanceId, computer.runtime, runtime, true);
      const authority = getClientConnectionAuthority(instanceId, resolvedRuntime.incarnation);
      return projectRuntime({
        ...computer,
        connection_profile: instance?.connectionProfile,
      }, resolvedRuntime,
      authority?.present
        ?? (computer.runtime.incarnation === resolvedRuntime.incarnation
          ? computer.client_connection_present ?? computer.connected
          : false),
      authority?.context
        ?? (computer.runtime.incarnation === resolvedRuntime.incarnation
          ? computer.connection_context
          : null),
      authority?.revision ?? computer.connection_revision);
    });
    return {
      data: withRuntimeCounts({ ...state.data, computers }),
    };
  }),
}));
