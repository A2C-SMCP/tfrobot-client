import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { formatRuntimeActionError } from '@/utils/runtimeActionError';
import {
  getClientConnectionAuthority,
  setClientConnectionAuthority,
  type ConnectionStateSummary,
} from './connectionAuthority';
import {
  isRuntimeSnapshotInstanceDeleted,
  projectRuntimeSnapshot,
  resolveRuntimeSnapshot,
  type ComputerRuntimeSnapshot,
} from './runtimeSnapshot';

export {
  isRuntimeRunning,
  isRuntimeTransportConnected,
  type ComputerRuntimeActionCapabilities,
  type ComputerRuntimeLifecycle,
  type ComputerRuntimeSnapshot,
} from './runtimeSnapshot';
export type { ConnectionStateSummary } from './connectionAuthority';

export type ComputerStatus = 'running' | 'stopped' | 'error';
export type ComputerConnectionStatus = 'connected' | 'disconnected';

export interface RobotBindingMetadata {
  employee_id: number;
  robot_id?: string;
  robot_account_id?: number;
  namespace?: string;
  robot_name?: string;
}

export type ComputerConnectionTargetType = 'manager_robot' | 'manual_smcp';

export interface ComputerConnectionTarget {
  type: ComputerConnectionTargetType;
  id: string;
  robotAccountId?: number;
}

export interface ComputerConnectionPolicy {
  target?: ComputerConnectionTarget | null;
  auto_connect: boolean;
}

export interface ComputerInstanceStatus {
  id: string;
  name: string;
  description?: string;
  local_skills_root?: string | null;
  effective_skill_home?: string;
  running: boolean;
  runtime: ComputerRuntimeSnapshot;
  connected: boolean;
  client_connection_present?: boolean;
  connection_revision?: number;
  connection_context?: ConnectionStateSummary | null;
  mcp_server_count: number;
  robot_binding?: RobotBindingMetadata | null;
  connection_policy?: ComputerConnectionPolicy;
  connection?: ConnectionStateSummary | null;
}

export interface ComputerInstance {
  id: string;
  name: string;
  description?: string;
  status: ComputerStatus;
  connectionStatus: ComputerConnectionStatus;
  /** Client-owned logical connection authority, independent of SDK transport lifecycle. */
  clientConnectionPresent?: boolean;
  clientConnectionContext?: ConnectionStateSummary | null;
  connectionProfile?: string;
  connectionUrl?: string;
  robotName?: string;
  robotBinding?: RobotBindingMetadata | null;
  localSkillsRoot?: string | null;
  effectiveSkillHome?: string;
  connectionPolicy: ComputerConnectionPolicy;
  mcpServerCount: number;
  runtime: ComputerRuntimeSnapshot;
}

export interface ComputerFormValues {
  name: string;
  description?: string;
}

export type DuplicateSkillHomeMode = 'empty' | 'copy';

export interface DuplicateComputerValues extends ComputerFormValues {
  sourceId: string;
  copyRobotBinding: boolean;
  connectionTargetId?: string;
  skillHomeMode: DuplicateSkillHomeMode;
}

interface ComputerState {
  instances: ComputerInstance[];
  loading: boolean;
  error: string | null;
  selectedInstanceId: string | null;
  listRequestId: number;
  mutationEpoch: number;
  profileMutationVersions: Record<string, number>;
  fetchInstances: () => Promise<void>;
  createInstance: (values: ComputerFormValues) => Promise<ComputerInstance>;
  updateInstance: (id: string, values: ComputerFormValues) => Promise<ComputerInstance>;
  duplicateInstance: (values: DuplicateComputerValues) => Promise<ComputerInstance>;
  deleteInstance: (id: string) => Promise<void>;
  startInstance: (id: string) => Promise<ComputerInstance>;
  stopInstance: (id: string) => Promise<ComputerInstance>;
  restartInstance: (id: string) => Promise<ComputerInstance>;
  reloadRuntime: (id: string) => Promise<ComputerInstance>;
  applyRuntimeSnapshot: (id: string, runtime: ComputerRuntimeSnapshot) => void;
  updateConnectionPolicy: (
    id: string,
    policy: ComputerConnectionPolicy,
  ) => Promise<ComputerInstance>;
  updateSkillHome: (id: string, localSkillsRoot?: string | null) => Promise<ComputerInstance>;
  connectSelectedTarget: (id: string) => Promise<void>;
  disconnectConnection: (id: string) => Promise<void>;
  selectInstance: (id: string) => void;
  reset: () => void;
}

const initialState = {
  instances: [] as ComputerInstance[],
  loading: false,
  error: null as string | null,
  selectedInstanceId: null as string | null,
  listRequestId: 0,
  mutationEpoch: 0,
  profileMutationVersions: {} as Record<string, number>,
};

function toComputerInstance(status: ComputerInstanceStatus): ComputerInstance {
  const runtime = status.runtime;
  const authority = getClientConnectionAuthority(status.id, runtime.incarnation);
  const fallbackContext = status.connection_context ?? status.connection ?? null;
  const clientConnectionPresent = authority?.present
    ?? status.client_connection_present
    ?? (fallbackContext ? true : status.connected);
  const connectionContext = authority?.context ?? (clientConnectionPresent ? fallbackContext : null);
  const projection = projectRuntimeSnapshot(runtime, clientConnectionPresent);
  return {
    id: status.id,
    name: status.name,
    description: status.description ?? undefined,
    status: projection.status,
    connectionStatus: projection.businessConnected ? 'connected' : 'disconnected',
    clientConnectionPresent,
    clientConnectionContext: connectionContext,
    connectionProfile: connectionContext?.profile_name,
    connectionUrl: connectionContext?.url,
    robotName: status.robot_binding?.robot_name,
    robotBinding: status.robot_binding,
    localSkillsRoot: status.local_skills_root ?? null,
    effectiveSkillHome: status.effective_skill_home,
    connectionPolicy: status.connection_policy ?? { target: null, auto_connect: false },
    mcpServerCount: projection.mcpServerCount,
    runtime,
  };
}

function normalizeFormValues(values: ComputerFormValues): ComputerFormValues {
  const name = values.name.trim();
  const description = values.description?.trim();
  return {
    name,
    description: description || undefined,
  };
}

async function ingestStatus(status: ComputerInstanceStatus): Promise<ComputerInstance> {
  const { useRuntimeStore } = await import('./runtimeStore');
  const connectionContext = status.connection_context ?? status.connection ?? null;
  setClientConnectionAuthority(
    status.id,
    status.client_connection_present ?? (connectionContext ? true : status.connected),
    connectionContext,
    status.connection_revision ?? 0,
    status.runtime,
  );
  useRuntimeStore.getState().receiveSnapshot(status.id, status.runtime);
  return toComputerInstance(status);
}

async function ingestStatuses(statuses: ComputerInstanceStatus[]): Promise<void> {
  const { useRuntimeStore } = await import('./runtimeStore');
  for (const status of statuses) {
    const connectionContext = status.connection_context ?? status.connection ?? null;
    setClientConnectionAuthority(
      status.id,
      status.client_connection_present ?? (connectionContext ? true : status.connected),
      connectionContext,
      status.connection_revision ?? 0,
      status.runtime,
    );
    useRuntimeStore.getState().receiveSnapshot(status.id, status.runtime);
  }
}

function requireInstance(instances: ComputerInstance[], id: string): ComputerInstance {
  const instance = instances.find((item) => item.id === id);
  if (!instance) throw new Error(`Computer instance no longer exists: ${id}`);
  return instance;
}

function mergeInstanceRuntime(
  current: ComputerInstance | undefined,
  incoming: ComputerInstance,
  eventSnapshot = false,
): ComputerInstance {
  const runtime = resolveRuntimeSnapshot(
    incoming.id,
    current?.runtime,
    incoming.runtime,
    eventSnapshot,
  );
  const authority = getClientConnectionAuthority(incoming.id, runtime.incarnation);
  const incarnationChangedWithoutAuthority = eventSnapshot
    && current?.runtime.incarnation !== runtime.incarnation
    && !authority;
  const clientConnectionPresent = authority?.present
    ?? (incarnationChangedWithoutAuthority
      ? false
      : incoming.clientConnectionPresent
        ?? current?.clientConnectionPresent
        ?? incoming.connectionStatus === 'connected');
  const clientConnectionContext = authority?.context
    ?? (clientConnectionPresent && !incarnationChangedWithoutAuthority
      ? incoming.clientConnectionContext ?? current?.clientConnectionContext ?? null
      : null);
  const projection = projectRuntimeSnapshot(runtime, clientConnectionPresent);
  return {
    ...incoming,
    runtime,
    status: projection.status,
    connectionStatus: projection.businessConnected ? 'connected' : 'disconnected',
    clientConnectionPresent,
    clientConnectionContext,
    connectionProfile: clientConnectionContext?.profile_name ?? incoming.connectionProfile,
    connectionUrl: clientConnectionContext?.url ?? incoming.connectionUrl,
    mcpServerCount: projection.mcpServerCount,
  };
}

function upsertInstance(instances: ComputerInstance[], instance: ComputerInstance): ComputerInstance[] {
  if (isRuntimeSnapshotInstanceDeleted(instance.id)) return instances;
  const current = instances.find((item) => item.id === instance.id);
  const merged = mergeInstanceRuntime(current, instance);
  if (!current) return [...instances, merged];
  return instances.map((item) => (item.id === instance.id ? merged : item));
}

function upsertRuntimeAction(
  instances: ComputerInstance[],
  incoming: ComputerInstance,
): ComputerInstance[] {
  if (isRuntimeSnapshotInstanceDeleted(incoming.id)) return instances;
  const current = instances.find((item) => item.id === incoming.id);
  if (!current) return instances;
  const merged = mergeInstanceRuntime(current, { ...current, runtime: incoming.runtime });
  return instances.map((item) => (item.id === incoming.id ? merged : item));
}

function reconcileInstances(
  current: ComputerInstance[],
  statuses: ComputerInstanceStatus[],
): ComputerInstance[] {
  return statuses
    .filter((status) => !isRuntimeSnapshotInstanceDeleted(status.id))
    .map((status) => {
    const incoming = toComputerInstance(status);
    return mergeInstanceRuntime(
      current.find((instance) => instance.id === incoming.id),
      incoming,
    );
    });
}

export const useComputerStore = create<ComputerState>((set, get) => ({
  ...initialState,

  fetchInstances: async () => {
    const listRequestId = get().listRequestId + 1;
    const mutationEpoch = get().mutationEpoch;
    set({ loading: true, error: null, listRequestId });
    try {
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      if (get().listRequestId !== listRequestId || get().mutationEpoch !== mutationEpoch) {
        set((state) => state.listRequestId === listRequestId ? { loading: false } : {});
        return;
      }
      const statusIds = new Set(statuses.map((status) => status.id));
      const missingIds = get().instances
        .filter((instance) => !statusIds.has(instance.id))
        .map((instance) => instance.id);
      if (missingIds.length > 0) {
        const { useRuntimeStore } = await import('./runtimeStore');
        for (const id of missingIds) useRuntimeStore.getState().forgetSnapshot(id);
      }
      await ingestStatuses(statuses);
      set((state) => {
        if (state.listRequestId !== listRequestId || state.mutationEpoch !== mutationEpoch) {
          return state.listRequestId === listRequestId ? { loading: false } : {};
        }
        const instances = reconcileInstances(state.instances, statuses);
        return {
          instances,
          loading: false,
          selectedInstanceId: instances.some((instance) => instance.id === state.selectedInstanceId)
            ? state.selectedInstanceId
            : instances[0]?.id ?? null,
        };
      });
    } catch (e) {
      set((state) => state.listRequestId === listRequestId
        ? { error: String(e), loading: false }
        : {});
    }
  },

  createInstance: async (values) => {
    set((state) => ({ loading: true, error: null, mutationEpoch: state.mutationEpoch + 1 }));
    try {
      const created = await ingestStatus(await invoke<ComputerInstanceStatus>('create_computer_instance', {
        request: normalizeFormValues(values),
      }));
      set((state) => ({
        instances: upsertInstance(state.instances, created),
        selectedInstanceId: created.id,
        loading: false,
      }));
      return requireInstance(get().instances, created.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  updateInstance: async (id, values) => {
    const profileVersion = (get().profileMutationVersions[id] ?? 0) + 1;
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      profileMutationVersions: { ...state.profileMutationVersions, [id]: profileVersion },
    }));
    try {
      const updated = await ingestStatus(await invoke<ComputerInstanceStatus>('rename_computer_instance', {
        request: { id, ...normalizeFormValues(values) },
      }));
      set((state) => state.profileMutationVersions[id] === profileVersion ? {
        instances: upsertInstance(state.instances, updated),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      } : { loading: false });
      return requireInstance(get().instances, updated.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  duplicateInstance: async (values) => {
    set((state) => ({ loading: true, error: null, mutationEpoch: state.mutationEpoch + 1 }));
    try {
      const request = {
        sourceId: values.sourceId,
        ...normalizeFormValues(values),
        copyRobotBinding: values.copyRobotBinding,
        connectionTargetId: values.connectionTargetId || undefined,
        skillHomeMode: values.skillHomeMode,
      };
      const duplicated = await ingestStatus(await invoke<ComputerInstanceStatus>('duplicate_computer_instance', {
        request,
      }));
      set((state) => ({
        instances: upsertInstance(state.instances, duplicated),
        selectedInstanceId: duplicated.id,
        loading: false,
      }));
      return requireInstance(get().instances, duplicated.id);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deleteInstance: async (id) => {
    const deletedIncarnation = requireInstance(get().instances, id).runtime.incarnation;
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
    }));
    try {
      await invoke('delete_computer_instance', { id });
      const { useRuntimeStore } = await import('./runtimeStore');
      useRuntimeStore.getState().evictSnapshot(id, deletedIncarnation);
      set((state) => {
        const instances = state.instances.filter((instance) => instance.id !== id);
        return {
          instances,
          selectedInstanceId: state.selectedInstanceId === id
            ? instances[0]?.id ?? null
            : state.selectedInstanceId,
          loading: false,
        };
      });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  startInstance: async (id) => {
    set({ loading: true, error: null });
    try {
      const started = await ingestStatus(await invoke<ComputerInstanceStatus>('start_computer_instance', { id }));
      set((state) => ({
        instances: upsertRuntimeAction(state.instances, started),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return requireInstance(get().instances, started.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  stopInstance: async (id) => {
    set({ loading: true, error: null });
    try {
      const stopped = await ingestStatus(await invoke<ComputerInstanceStatus>('stop_computer_instance', { id }));
      set((state) => ({
        instances: upsertRuntimeAction(state.instances, stopped),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return requireInstance(get().instances, stopped.id);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  restartInstance: async (id) => {
    set({ loading: true, error: null });
    try {
      const restarted = await ingestStatus(await invoke<ComputerInstanceStatus>('restart_computer_instance', { id }));
      set((state) => ({
        instances: upsertRuntimeAction(state.instances, restarted),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return requireInstance(get().instances, restarted.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  reloadRuntime: async (id) => {
    set({ loading: true, error: null });
    try {
      const reloaded = await ingestStatus(await invoke<ComputerInstanceStatus>('reload_computer_runtime', { id }));
      set((state) => ({
        instances: upsertRuntimeAction(state.instances, reloaded),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return requireInstance(get().instances, reloaded.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  applyRuntimeSnapshot: (id, runtime) => set((state) => ({
    instances: state.instances.map((instance) => instance.id === id
      ? mergeInstanceRuntime(instance, { ...instance, runtime }, true)
      : instance),
  })),

  updateConnectionPolicy: async (id, policy) => {
    const profileVersion = (get().profileMutationVersions[id] ?? 0) + 1;
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      profileMutationVersions: { ...state.profileMutationVersions, [id]: profileVersion },
    }));
    try {
      const updated = await ingestStatus(await invoke<ComputerInstanceStatus>('update_computer_connection_policy', {
        request: {
          id,
          target: policy.target ?? null,
          autoConnect: policy.auto_connect,
        },
      }));
      set((state) => state.profileMutationVersions[id] === profileVersion ? {
        instances: upsertInstance(state.instances, updated),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      } : { loading: false });
      return requireInstance(get().instances, updated.id);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateSkillHome: async (id, localSkillsRoot) => {
    const profileVersion = (get().profileMutationVersions[id] ?? 0) + 1;
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      profileMutationVersions: { ...state.profileMutationVersions, [id]: profileVersion },
    }));
    try {
      const updated = await ingestStatus(await invoke<ComputerInstanceStatus>('update_computer_skill_home', {
        request: {
          id,
          localSkillsRoot: localSkillsRoot || null,
        },
      }));
      set((state) => state.profileMutationVersions[id] === profileVersion ? {
        instances: upsertInstance(state.instances, updated),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      } : { loading: false });
      return requireInstance(get().instances, updated.id);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  connectSelectedTarget: async (id) => {
    const mutationEpoch = get().mutationEpoch;
    set({ loading: true, error: null });
    try {
      await invoke('connect_computer_connection_target', { id });
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      if (get().mutationEpoch !== mutationEpoch) {
        set({ loading: false });
        return;
      }
      await ingestStatuses(statuses);
      set((state) => {
        if (state.mutationEpoch !== mutationEpoch) return { loading: false };
        const instances = reconcileInstances(state.instances, statuses);
        return {
          instances,
          selectedInstanceId: instances.some((instance) => instance.id === state.selectedInstanceId)
            ? state.selectedInstanceId
            : instances[0]?.id ?? null,
          loading: false,
        };
      });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  disconnectConnection: async (id) => {
    const mutationEpoch = get().mutationEpoch;
    set({ loading: true, error: null });
    try {
      await invoke('disconnect_computer_connection_target', { id });
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      if (get().mutationEpoch !== mutationEpoch) {
        set({ loading: false });
        return;
      }
      await ingestStatuses(statuses);
      set((state) => {
        if (state.mutationEpoch !== mutationEpoch) return { loading: false };
        const instances = reconcileInstances(state.instances, statuses);
        return {
          instances,
          selectedInstanceId: instances.some((instance) => instance.id === state.selectedInstanceId)
            ? state.selectedInstanceId
            : instances[0]?.id ?? null,
          loading: false,
        };
      });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  selectInstance: (id: string) => set({ selectedInstanceId: id }),

  reset: () => set(initialState),
}));
