import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { create } from 'zustand';
import { useComputerStore } from './computerStore';
import { useDashboardStore } from './dashboardStore';
import { useDebugStore } from './debugStore';
import { useMcpStore } from './mcpStore';
import { useSdkConfigStore } from './sdkConfigStore';
import { useSkillStore } from './skillStore';
import {
  clearClientConnectionAuthority,
  resetClientConnectionAuthorities,
  setClientConnectionAuthority,
  type ClientConnectionAuthority,
  type ClientConnectionAuthorityInput,
} from './connectionAuthority';
import { useConnectionStore } from './connectionStore';
import {
  acceptAuthoritativeRuntimeSnapshot,
  canProjectRuntimeIncarnation,
  clearAuthoritativeRuntimeSnapshots,
  evictAuthoritativeRuntimeSnapshot,
  forgetAuthoritativeRuntimeSnapshot,
  type ComputerRuntimeSnapshot,
} from './runtimeSnapshot';

export const COMPUTER_RUNTIME_STATUS_EVENT = 'computer-runtime-status';

export type ComputerRuntimeEventCause =
  | { kind: 'lifecycle_changed'; state: ComputerRuntimeSnapshot['lifecycle'] }
  | { kind: 'config_revision_bumped'; revision: number }
  | { kind: 'capability_revision_bumped'; revision: number }
  | {
      kind: 'client_connection_state_changed';
      revision: number;
      status: ClientConnectionAuthority['status'];
    }
  /** @deprecated Compatibility with runtime events emitted before TFRC-73. */
  | { kind: 'client_connection_authority_changed'; revision: number; present: boolean }
  | { kind: 'client_diagnostic_changed'; operation: string; has_error: boolean }
  | {
      kind: 'mcp_diagnostic_changed';
      bundle_id: string;
      operation: string;
      has_error: boolean;
    }
  | { kind: 'handle_replaced'; reason: string }
  | { kind: 'observation_advanced' }
  | { kind: 'resync'; skipped_events: number };

export interface ComputerRuntimeStatusEvent {
  instance_id: string;
  cause: ComputerRuntimeEventCause;
  snapshot: ComputerRuntimeSnapshot;
  connection: ClientConnectionAuthorityInput;
}

export interface ComputerRuntimeEventRecord extends ComputerRuntimeStatusEvent {
  received_at: string;
}

export const RUNTIME_EVENT_HISTORY_LIMIT = 50;

interface ComputerRuntimeSnapshotRecord {
  instance_id: string;
  snapshot: ComputerRuntimeSnapshot;
  connection: ClientConnectionAuthorityInput;
}

interface RuntimeState {
  snapshots: Record<string, ComputerRuntimeSnapshot>;
  eventsByInstance: Record<string, ComputerRuntimeEventRecord[]>;
  initialized: boolean;
  error: string | null;
  initialize: () => Promise<void>;
  resync: () => Promise<void>;
  recover: () => Promise<void>;
  dispose: () => Promise<void>;
  receiveEvent: (event: ComputerRuntimeStatusEvent) => void;
  receiveSnapshot: (
    instanceId: string,
    snapshot: ComputerRuntimeSnapshot,
    connection?: ClientConnectionAuthorityInput,
    options?: { allowDeletedRediscovery?: boolean },
  ) => void;
  evictSnapshot: (instanceId: string, incarnation: number) => void;
  forgetSnapshot: (instanceId: string) => void;
  reset: () => void;
}

let unlisten: UnlistenFn | null = null;
let initializePromise: Promise<void> | null = null;
let lifecycleEpoch = 0;

function refreshRevisionConsumers(
  instanceId: string,
  previous: ComputerRuntimeSnapshot | undefined,
  next: ComputerRuntimeSnapshot,
) {
  const handleChanged = !previous
    || previous.incarnation !== next.incarnation
    || previous.generation !== next.generation;
  const configChanged = handleChanged || previous.config_revision !== next.config_revision;
  const capabilityChanged = handleChanged
    || previous.capability_revision !== next.capability_revision;

  if (configChanged || capabilityChanged) {
    const mcp = useMcpStore.getState();
    if (mcp.activeInstanceId === instanceId) void mcp.fetchServers(instanceId);
  }

  if (configChanged) {
    const sdkConfig = useSdkConfigStore.getState();
    if (sdkConfig.activeInstanceId === instanceId) void sdkConfig.fetchConfig(instanceId);
  }

  if (capabilityChanged) {
    const debug = useDebugStore.getState();
    if (debug.activeInstanceId === instanceId) void debug.fetchTools(instanceId);

    const skills = useSkillStore.getState();
    if (skills.activeInstanceId === instanceId) {
      void skills.fetchSkills(instanceId);
      void skills.fetchMarketplaceGovernance(instanceId);
    }
  }
}

function applySnapshotToConsumers(instanceId: string, snapshot: ComputerRuntimeSnapshot) {
  useComputerStore.getState().applyRuntimeSnapshot(instanceId, snapshot);
  useDashboardStore.getState().applyRuntimeSnapshot(instanceId, snapshot);
  useConnectionStore.getState().applyRuntimeSnapshot(instanceId, snapshot);
}

function applyConnectionAuthority(
  instanceId: string,
  snapshot: ComputerRuntimeSnapshot,
  connection: ClientConnectionAuthorityInput,
) {
  if (!canProjectRuntimeIncarnation(instanceId, snapshot.incarnation)) return;
  setClientConnectionAuthority(instanceId, connection, snapshot);
}

const initialState = {
  snapshots: {} as Record<string, ComputerRuntimeSnapshot>,
  eventsByInstance: {} as Record<string, ComputerRuntimeEventRecord[]>,
  initialized: false,
  error: null as string | null,
};

export const useRuntimeStore = create<RuntimeState>((set, get) => ({
  ...initialState,

  reset: () => {
    clearAuthoritativeRuntimeSnapshots();
    resetClientConnectionAuthorities();
    set(initialState);
  },

  receiveSnapshot: (instanceId, snapshot, connection, options) => {
    const { accepted, previous } = acceptAuthoritativeRuntimeSnapshot(
      instanceId,
      snapshot,
      options,
    );
    // Admission runs first so a trusted post-delete status can explicitly reopen the event fence
    // before its independently versioned connection authority is projected.
    if (connection) applyConnectionAuthority(instanceId, snapshot, connection);
    if (!accepted) {
      // A status response may carry a newer independent connection authority together with an
      // older SDK snapshot. Re-project the current SDK authority so that connection-only changes
      // reach every consumer even when the paired runtime snapshot is rejected.
      if (previous) applySnapshotToConsumers(instanceId, previous);
      return;
    }
    set((state) => {
      const incarnationChanged = previous && previous.incarnation !== snapshot.incarnation;
      if (!incarnationChanged) {
        return { snapshots: { ...state.snapshots, [instanceId]: snapshot } };
      }
      const eventsByInstance = { ...state.eventsByInstance };
      delete eventsByInstance[instanceId];
      return {
        snapshots: { ...state.snapshots, [instanceId]: snapshot },
        eventsByInstance,
      };
    });
    applySnapshotToConsumers(instanceId, snapshot);
    refreshRevisionConsumers(instanceId, previous, snapshot);
  },

  receiveEvent: (event) => {
    const { accepted, previous } = acceptAuthoritativeRuntimeSnapshot(
      event.instance_id,
      event.snapshot,
    );
    // Runtime admission establishes the observation fence first. Connection authority then uses
    // its own revision plus the paired runtime generation/revision to reject equal-revision
    // observations that arrive out of order.
    applyConnectionAuthority(event.instance_id, event.snapshot, event.connection);
    if (!accepted) {
      // Connection authority is versioned independently and has already been projected above.
      // Event history deliberately remains a history of accepted runtime observations so a
      // connection update paired with an older SDK snapshot cannot reintroduce stale snapshots.
      if (previous) applySnapshotToConsumers(event.instance_id, previous);
      return;
    }
    const record: ComputerRuntimeEventRecord = {
      ...event,
      received_at: new Date().toISOString(),
    };
    set((state) => {
      const priorEvents = previous && previous.incarnation === event.snapshot.incarnation
        ? state.eventsByInstance[event.instance_id] ?? []
        : [];
      return {
        snapshots: { ...state.snapshots, [event.instance_id]: event.snapshot },
        eventsByInstance: {
          ...state.eventsByInstance,
          [event.instance_id]: [...priorEvents, record].slice(-RUNTIME_EVENT_HISTORY_LIMIT),
        },
      };
    });
    applySnapshotToConsumers(event.instance_id, event.snapshot);
    refreshRevisionConsumers(event.instance_id, previous, event.snapshot);
  },

  evictSnapshot: (instanceId, incarnation) => {
    evictAuthoritativeRuntimeSnapshot(instanceId, incarnation);
    clearClientConnectionAuthority(instanceId);
    useConnectionStore.getState().forgetStatus(instanceId);
    set((state) => {
      const snapshots = { ...state.snapshots };
      const eventsByInstance = { ...state.eventsByInstance };
      delete snapshots[instanceId];
      delete eventsByInstance[instanceId];
      return { snapshots, eventsByInstance };
    });
  },

  forgetSnapshot: (instanceId) => {
    forgetAuthoritativeRuntimeSnapshot(instanceId);
    clearClientConnectionAuthority(instanceId);
    useConnectionStore.getState().forgetStatus(instanceId);
    set((state) => {
      const snapshots = { ...state.snapshots };
      const eventsByInstance = { ...state.eventsByInstance };
      delete snapshots[instanceId];
      delete eventsByInstance[instanceId];
      return { snapshots, eventsByInstance };
    });
  },

  initialize: async () => {
    if (get().initialized) return;
    if (initializePromise) return initializePromise;
    const epoch = ++lifecycleEpoch;
    const session = { promise: null as Promise<void> | null };
    let releaseListener: UnlistenFn | null = null;
    const promise = Promise.resolve().then(async () => {
      try {
        const rawUnlisten = await listen<ComputerRuntimeStatusEvent>(
          COMPUTER_RUNTIME_STATUS_EVENT,
          ({ payload }) => useRuntimeStore.getState().receiveEvent(payload),
        );
        let listenerActive = true;
        releaseListener = () => {
          if (!listenerActive) return;
          listenerActive = false;
          rawUnlisten();
        };
        if (epoch !== lifecycleEpoch) {
          releaseListener();
          return;
        }
        unlisten = releaseListener;

        const records = await invoke<ComputerRuntimeSnapshotRecord[]>('enable_computer_runtime_events');
        if (epoch !== lifecycleEpoch) {
          if (unlisten === releaseListener) unlisten = null;
          releaseListener();
          return;
        }
        for (const record of records) {
          useRuntimeStore.getState().receiveSnapshot(
            record.instance_id,
            record.snapshot,
            record.connection,
          );
        }
        set({ initialized: true, error: null });
      } catch (error) {
        if (unlisten === releaseListener) unlisten = null;
        releaseListener?.();
        if (epoch !== lifecycleEpoch) {
          return;
        }
        set({ initialized: false, error: String(error) });
        throw error;
      } finally {
        if (initializePromise === session.promise) initializePromise = null;
      }
    });
    session.promise = promise;
    initializePromise = promise;
    return promise;
  },

  resync: async () => {
    try {
      const records = await invoke<ComputerRuntimeSnapshotRecord[]>('get_computer_runtime_snapshots');
      for (const record of records) {
        get().receiveSnapshot(record.instance_id, record.snapshot, record.connection);
      }
      set({ error: null });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  recover: async () => {
    if (!get().initialized) await get().initialize();
    await get().resync();
  },

  dispose: async () => {
    lifecycleEpoch += 1;
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
    initializePromise = null;
    set({ initialized: false });
  },
}));
