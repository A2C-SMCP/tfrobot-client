import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { formatRuntimeActionError } from '@/utils/runtimeActionError';
import {
  getClientConnectionAuthority,
  legacyClientConnectionState,
  type ClientConnectionAuthority,
  type ClientConnectionStatus,
  type ConnectionStateSummary,
} from './connectionAuthority';
import {
  isRuntimeSnapshotInstanceDeleted,
  projectRuntimeSnapshot,
  resolveRuntimeSnapshot,
  type ComputerRuntimeSnapshot,
} from './runtimeSnapshot';
import type { ComputerRuntimeUserState } from './runtimeSnapshot';
import type { ManagerContextKey } from './managerStore';

export {
  isRuntimeRunning,
  isRuntimeTransportConnected,
  type ComputerRuntimeActionCapabilities,
  type ComputerRuntimeLifecycle,
  type ComputerRuntimeUserState,
  type ComputerRuntimeSnapshot,
} from './runtimeSnapshot';
export type { ConnectionStateSummary } from './connectionAuthority';

export type ComputerStatus = ComputerRuntimeUserState;
export type ComputerConnectionStatus = ClientConnectionStatus;

export interface RobotBindingMetadata {
  context_key?: ManagerContextKey;
  state: 'active' | 'dormant' | 'needs_rebind';
  employee_id: number;
  robot_id?: string;
  last_resolved_robot_account_id?: string;
  namespace?: string;
  robot_name?: string;
}

export type ComputerConnectionTargetType = 'manager_robot' | 'manual_smcp';

export type ComputerConnectionTarget =
  | {
    type: 'manager_robot';
    contextKey: ManagerContextKey;
    employeeId: number;
    lastResolvedRobotAccountId?: string;
  }
  | {
    type: 'manual_smcp';
    id: string;
  };

export interface ComputerConnectionPolicy {
  target?: ComputerConnectionTarget | null;
  auto_connect: boolean;
}

export interface ComputerInstanceStatus {
  id: string;
  name: string;
  description?: string;
  local_skills_root?: string | null;
  default_skill_home: string;
  configured_skill_home: string;
  effective_skill_home: string;
  running: boolean;
  runtime: ComputerRuntimeSnapshot;
  connection_state?: ClientConnectionAuthority;
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
  /** Canonical backend-owned connection operation state and capabilities. */
  connectionState?: ClientConnectionAuthority;
  /** Client-owned logical connection authority, independent of SDK transport lifecycle. */
  clientConnectionPresent?: boolean;
  clientConnectionContext?: ConnectionStateSummary | null;
  connectionProfile?: string;
  connectionUrl?: string;
  robotName?: string;
  robotBinding?: RobotBindingMetadata | null;
  localSkillsRoot?: string | null;
  defaultSkillHome?: string;
  configuredSkillHome?: string;
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
  skillHomeMode: DuplicateSkillHomeMode;
}

interface ComputerState {
  instances: ComputerInstance[];
  loading: boolean;
  error: string | null;
  selectedInstanceId: string | null;
  pendingMutationCount: number;
  listRequestId: number;
  mutationEpoch: number;
  mutationCompletionRevision: number;
  deletionRevision: number;
  profileMutationVersions: Record<string, number>;
  connectionMetadataRequestIds: Record<string, number>;
  fetchInstances: () => Promise<void>;
  reconcileConnectionMetadata: (id: string) => Promise<void>;
  createInstance: (values: ComputerFormValues) => Promise<ComputerInstance>;
  updateInstance: (id: string, values: ComputerFormValues) => Promise<ComputerInstance>;
  duplicateInstance: (values: DuplicateComputerValues) => Promise<ComputerInstance>;
  deleteInstance: (id: string) => Promise<void>;
  startInstance: (id: string) => Promise<ComputerInstance>;
  stopInstance: (id: string) => Promise<ComputerInstance>;
  restartInstance: (id: string) => Promise<ComputerInstance>;
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
  pendingMutationCount: 0,
  listRequestId: 0,
  mutationEpoch: 0,
  mutationCompletionRevision: 0,
  deletionRevision: 0,
  profileMutationVersions: {} as Record<string, number>,
  connectionMetadataRequestIds: {} as Record<string, number>,
};

function connectionStateFromStatus(status: ComputerInstanceStatus): ClientConnectionAuthority {
  if (status.connection_state) return status.connection_state;
  const context = status.connection_context ?? status.connection ?? null;
  const connectionState = legacyClientConnectionState(
    status.client_connection_present ?? (context ? true : status.connected),
    context,
    status.connection_revision ?? 0,
    status.runtime,
  );
  return connectionState;
}

function mergeConnectionMetadata(
  instance: ComputerInstance,
  status: ComputerInstanceStatus,
): ComputerInstance {
  const robotBinding = status.robot_binding === undefined
    ? instance.robotBinding
    : status.robot_binding;
  return {
    ...instance,
    robotBinding,
    robotName: robotBinding?.robot_name,
    connectionPolicy: status.connection_policy ?? instance.connectionPolicy,
  };
}

function toComputerInstance(status: ComputerInstanceStatus): ComputerInstance {
  const runtime = status.runtime;
  const authority = getClientConnectionAuthority(status.id, runtime.incarnation);
  const connectionState = authority
    ?? connectionStateFromStatus(status);
  const connectionContext = connectionState.context;
  const projection = projectRuntimeSnapshot(runtime, connectionState.status === 'connected');
  return {
    id: status.id,
    name: status.name,
    description: status.description ?? undefined,
    status: projection.status,
    connectionStatus: connectionState.status,
    connectionState,
    clientConnectionPresent: connectionState.present,
    clientConnectionContext: connectionContext,
    connectionProfile: connectionContext?.profile_name,
    connectionUrl: connectionContext?.url,
    robotName: status.robot_binding?.robot_name,
    robotBinding: status.robot_binding,
    localSkillsRoot: status.local_skills_root ?? null,
    defaultSkillHome: status.default_skill_home,
    configuredSkillHome: status.configured_skill_home,
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

async function ingestStatus(
  status: ComputerInstanceStatus,
  options?: { allowDeletedRediscovery?: boolean },
): Promise<ComputerInstance> {
  const { useRuntimeStore } = await import('./runtimeStore');
  useRuntimeStore.getState().receiveSnapshot(
    status.id,
    status.runtime,
    connectionStateFromStatus(status),
    options,
  );
  return toComputerInstance(status);
}

async function ingestStatuses(
  statuses: ComputerInstanceStatus[],
  canIngest: () => boolean,
): Promise<boolean> {
  const { useRuntimeStore } = await import('./runtimeStore');
  // The import above yields. Revalidate after it and immediately before any trusted
  // allowDeletedRediscovery side effect so a deletion committed in that window wins.
  if (!canIngest()) return false;
  for (const status of statuses) {
    useRuntimeStore.getState().receiveSnapshot(
      status.id,
      status.runtime,
      connectionStateFromStatus(status),
      { allowDeletedRediscovery: true },
    );
  }
  return true;
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
  const fallbackPresent = incarnationChangedWithoutAuthority
    ? false
    : incoming.clientConnectionPresent
      ?? current?.clientConnectionPresent
      ?? incoming.connectionStatus === 'connected';
  const fallbackContext = fallbackPresent && !incarnationChangedWithoutAuthority
    ? incoming.clientConnectionContext ?? current?.clientConnectionContext ?? null
    : null;
  const connectionState = authority
    ?? (!incarnationChangedWithoutAuthority
      ? incoming.connectionState ?? current?.connectionState
      : undefined)
    ?? legacyClientConnectionState(fallbackPresent, fallbackContext, 0, runtime);
  const projection = projectRuntimeSnapshot(runtime, connectionState.status === 'connected');
  return {
    ...incoming,
    runtime,
    status: projection.status,
    connectionStatus: connectionState.status,
    connectionState,
    clientConnectionPresent: connectionState.present,
    clientConnectionContext: connectionState.context,
    connectionProfile: connectionState.context?.profile_name ?? incoming.connectionProfile,
    connectionUrl: connectionState.context?.url ?? incoming.connectionUrl,
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
    const mutationCompletionRevision = get().mutationCompletionRevision;
    const deletionRevision = get().deletionRevision;
    const isCurrent = () => (
      get().listRequestId === listRequestId
      && get().mutationEpoch === mutationEpoch
      && get().mutationCompletionRevision === mutationCompletionRevision
      && get().deletionRevision === deletionRevision
    );
    const releaseStaleListLoading = () => set((state) => (
      state.listRequestId === listRequestId
        ? { loading: state.pendingMutationCount > 0 }
        : {}
    ));
    set({ loading: true, error: null, listRequestId });
    try {
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      if (!isCurrent()) {
        releaseStaleListLoading();
        return;
      }
      const statusIds = new Set(statuses.map((status) => status.id));
      const missingIds = get().instances
        .filter((instance) => !statusIds.has(instance.id))
        .map((instance) => instance.id);
      if (missingIds.length > 0) {
        const { useRuntimeStore } = await import('./runtimeStore');
        if (!isCurrent()) {
          releaseStaleListLoading();
          return;
        }
        for (const id of missingIds) useRuntimeStore.getState().forgetSnapshot(id);
      }
      if (!(await ingestStatuses(statuses, isCurrent))) {
        releaseStaleListLoading();
        return;
      }
      set((state) => {
        if (
          state.listRequestId !== listRequestId
          || state.mutationEpoch !== mutationEpoch
          || state.mutationCompletionRevision !== mutationCompletionRevision
          || state.deletionRevision !== deletionRevision
        ) {
          return state.listRequestId === listRequestId
            ? { loading: state.pendingMutationCount > 0 }
            : {};
        }
        const instances = reconcileInstances(state.instances, statuses);
        return {
          instances,
          loading: state.pendingMutationCount > 0,
          selectedInstanceId: instances.some((instance) => instance.id === state.selectedInstanceId)
            ? state.selectedInstanceId
            : instances[0]?.id ?? null,
        };
      });
    } catch (e) {
      set((state) => {
        if (state.listRequestId !== listRequestId) return {};
        if (
          state.mutationEpoch !== mutationEpoch
          || state.mutationCompletionRevision !== mutationCompletionRevision
          || state.deletionRevision !== deletionRevision
          || state.pendingMutationCount > 0
        ) return { loading: state.pendingMutationCount > 0 };
        return { error: String(e), loading: false };
      });
    }
  },

  reconcileConnectionMetadata: async (id) => {
    const requestId = (get().connectionMetadataRequestIds[id] ?? 0) + 1;
    const profileVersion = (get().profileMutationVersions[id] ?? 0) + 1;
    // Metadata persisted by a completed connection command is a profile mutation. Advance the
    // start fence so older list/profile responses cannot restore the previous binding or policy.
    // shared epoch so any list/profile response issued before this reconciliation cannot restore
    // the old binding or policy after the new metadata has been applied.
    const nextMutationEpoch = get().mutationEpoch + 1;
    set((state) => ({
      mutationEpoch: nextMutationEpoch,
      // This reconciliation is the newest profile observation for the instance. Invalidate
      // earlier rename/policy/skill-home responses that carry an older whole-status payload.
      profileMutationVersions: {
        ...state.profileMutationVersions,
        [id]: profileVersion,
      },
      connectionMetadataRequestIds: {
        ...state.connectionMetadataRequestIds,
        [id]: requestId,
      },
    }));
    try {
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      const status = statuses.find((candidate) => candidate.id === id);
      set((state) => {
        if (
          state.profileMutationVersions[id] !== profileVersion
          || state.connectionMetadataRequestIds[id] !== requestId
        ) return {};
        if (!status) return {};
        return {
          instances: state.instances.map((instance) => instance.id === id
            ? mergeConnectionMetadata(instance, status)
            : instance),
        };
      });
    } catch (error) {
      const current = (
        get().profileMutationVersions[id] === profileVersion
        && get().connectionMetadataRequestIds[id] === requestId
      );
      if (!current) return;
      const reconciliationError = new Error(
        `Connection succeeded, but refreshing its saved binding and policy failed: ${String(error)}`,
      );
      set((state) => (
        state.profileMutationVersions[id] === profileVersion
        && state.connectionMetadataRequestIds[id] === requestId
      ) ? { error: reconciliationError.message } : {});
      throw reconciliationError;
    } finally {
      // Reconciliation is itself a profile mutation. Invalidate any list that began while its
      // authoritative metadata request was in flight, regardless of success or failure.
      set((state) => ({
        mutationCompletionRevision: state.mutationCompletionRevision + 1,
      }));
    }
  },

  createInstance: async (values) => {
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      pendingMutationCount: state.pendingMutationCount + 1,
    }));
    try {
      const created = await ingestStatus(
        await invoke<ComputerInstanceStatus>('create_computer_instance', {
          request: normalizeFormValues(values),
        }),
        { allowDeletedRediscovery: true },
      );
      set((state) => ({
        instances: upsertInstance(state.instances, created),
        selectedInstanceId: created.id,
      }));
      return requireInstance(get().instances, created.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  updateInstance: async (id, values) => {
    const profileVersion = (get().profileMutationVersions[id] ?? 0) + 1;
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      pendingMutationCount: state.pendingMutationCount + 1,
      profileMutationVersions: { ...state.profileMutationVersions, [id]: profileVersion },
    }));
    try {
      const updated = await ingestStatus(await invoke<ComputerInstanceStatus>('rename_computer_instance', {
        request: { id, ...normalizeFormValues(values) },
      }));
      set((state) => state.profileMutationVersions[id] === profileVersion ? {
        instances: upsertInstance(state.instances, updated),
        selectedInstanceId: state.selectedInstanceId,
      } : {});
      return requireInstance(get().instances, updated.id);
    } catch (e) {
      set((state) => state.profileMutationVersions[id] === profileVersion
        ? { error: formatRuntimeActionError(e) }
        : {});
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  duplicateInstance: async (values) => {
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      pendingMutationCount: state.pendingMutationCount + 1,
    }));
    try {
      const request = {
        sourceId: values.sourceId,
        ...normalizeFormValues(values),
        copyRobotBinding: values.copyRobotBinding,
        skillHomeMode: values.skillHomeMode,
      };
      const duplicated = await ingestStatus(
        await invoke<ComputerInstanceStatus>('duplicate_computer_instance', { request }),
        { allowDeletedRediscovery: true },
      );
      set((state) => ({
        instances: upsertInstance(state.instances, duplicated),
        selectedInstanceId: duplicated.id,
      }));
      return requireInstance(get().instances, duplicated.id);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  deleteInstance: async (id) => {
    const deletedIncarnation = requireInstance(get().instances, id).runtime.incarnation;
    const profileVersion = (get().profileMutationVersions[id] ?? 0) + 1;
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      pendingMutationCount: state.pendingMutationCount + 1,
      profileMutationVersions: {
        ...state.profileMutationVersions,
        [id]: profileVersion,
      },
    }));
    try {
      await invoke('delete_computer_instance', { id });
      // Commit a second fence after the backend deletion succeeds. A list/profile request that
      // started while deletion was in flight must not be trusted to rediscover the instance.
      set((state) => ({
        mutationEpoch: state.mutationEpoch + 1,
        deletionRevision: state.deletionRevision + 1,
        profileMutationVersions: {
          ...state.profileMutationVersions,
          [id]: (state.profileMutationVersions[id] ?? profileVersion) + 1,
        },
      }));
      const { useRuntimeStore } = await import('./runtimeStore');
      useRuntimeStore.getState().evictSnapshot(id, deletedIncarnation);
      set((state) => {
        const instances = state.instances.filter((instance) => instance.id !== id);
        return {
          instances,
          selectedInstanceId: state.selectedInstanceId === id
            ? instances[0]?.id ?? null
            : state.selectedInstanceId,
        };
      });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  startInstance: async (id) => {
    set((state) => ({
      loading: true,
      error: null,
      pendingMutationCount: state.pendingMutationCount + 1,
    }));
    try {
      const started = await ingestStatus(await invoke<ComputerInstanceStatus>('start_computer_instance', { id }));
      set((state) => ({
        instances: upsertRuntimeAction(state.instances, started),
        selectedInstanceId: state.selectedInstanceId,
      }));
      return requireInstance(get().instances, started.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  stopInstance: async (id) => {
    set((state) => ({
      loading: true,
      error: null,
      pendingMutationCount: state.pendingMutationCount + 1,
    }));
    try {
      const stopped = await ingestStatus(await invoke<ComputerInstanceStatus>('stop_computer_instance', { id }));
      set((state) => ({
        instances: upsertRuntimeAction(state.instances, stopped),
        selectedInstanceId: state.selectedInstanceId,
      }));
      return requireInstance(get().instances, stopped.id);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  restartInstance: async (id) => {
    set((state) => ({
      loading: true,
      error: null,
      pendingMutationCount: state.pendingMutationCount + 1,
    }));
    try {
      const restarted = await ingestStatus(await invoke<ComputerInstanceStatus>('restart_computer_instance', { id }));
      set((state) => ({
        instances: upsertRuntimeAction(state.instances, restarted),
        selectedInstanceId: state.selectedInstanceId,
      }));
      return requireInstance(get().instances, restarted.id);
    } catch (e) {
      set({ error: formatRuntimeActionError(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
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
      pendingMutationCount: state.pendingMutationCount + 1,
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
      } : {});
      return requireInstance(get().instances, updated.id);
    } catch (e) {
      set((state) => state.profileMutationVersions[id] === profileVersion
        ? { error: String(e) }
        : {});
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  updateSkillHome: async (id, localSkillsRoot) => {
    const profileVersion = (get().profileMutationVersions[id] ?? 0) + 1;
    set((state) => ({
      loading: true,
      error: null,
      mutationEpoch: state.mutationEpoch + 1,
      pendingMutationCount: state.pendingMutationCount + 1,
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
      } : {});
      return requireInstance(get().instances, updated.id);
    } catch (e) {
      set((state) => state.profileMutationVersions[id] === profileVersion
        ? { error: String(e) }
        : {});
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  connectSelectedTarget: async (id) => {
    set((state) => ({
      loading: true,
      error: null,
      pendingMutationCount: state.pendingMutationCount + 1,
    }));
    try {
      await invoke('connect_computer_connection_target', { id });
      await get().reconcileConnectionMetadata(id);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  disconnectConnection: async (id) => {
    set((state) => ({
      loading: true,
      error: null,
      pendingMutationCount: state.pendingMutationCount + 1,
    }));
    try {
      await invoke('disconnect_computer_connection_target', { id });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    } finally {
      set((state) => {
        const pendingMutationCount = Math.max(0, state.pendingMutationCount - 1);
        return {
          pendingMutationCount,
          loading: pendingMutationCount > 0,
          mutationCompletionRevision: state.mutationCompletionRevision + 1,
        };
      });
    }
  },

  selectInstance: (id: string) => set({ selectedInstanceId: id }),

  reset: () => set(initialState),
}));
