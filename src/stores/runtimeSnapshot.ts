export type ComputerRuntimeLifecycle =
  | 'created'
  | 'starting'
  | 'started'
  | 'connecting'
  | 'connected'
  | 'joined_office'
  | 'syncing'
  | 'degraded'
  | 'disconnecting'
  | 'stopping'
  | 'stopped'
  | 'shutdown'
  | 'error';

export type ComputerRuntimeUserState =
  | 'not_running'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'degraded'
  | 'error';

export type ComputerRuntimeActionDisabledReason =
  | 'already_running'
  | 'not_running'
  | 'transition_in_progress'
  | 'degraded'
  | 'connection_unavailable';

export interface ComputerRuntimeActionCapability {
  enabled: boolean;
  disabled_reason?: ComputerRuntimeActionDisabledReason | null;
}

export interface ComputerRuntimeActionCapabilities {
  start: ComputerRuntimeActionCapability;
  stop: ComputerRuntimeActionCapability;
  restart: ComputerRuntimeActionCapability;
  connect: ComputerRuntimeActionCapability;
  disconnect: ComputerRuntimeActionCapability;
  manage_mcp: ComputerRuntimeActionCapability;
}

export interface ComputerRuntimeSnapshot {
  incarnation: number;
  generation: number;
  snapshot_revision: number;
  lifecycle: ComputerRuntimeLifecycle;
  user_state: ComputerRuntimeUserState;
  actions: ComputerRuntimeActionCapabilities;
  config_revision: number;
  capability_revision: number;
  mcp_servers: number;
  active_mcp_servers: number;
  tools: number;
  skills: number;
  last_error?: string | null;
  degraded_reason?: string | null;
}

const authoritativeSnapshots = new Map<string, ComputerRuntimeSnapshot>();
const deletedIncarnations = new Map<string, number>();
const retiredIncarnations = new Map<string, number>();

export function isRuntimeRunning(runtime: ComputerRuntimeSnapshot): boolean {
  return [
    'starting',
    'started',
    'connecting',
    'connected',
    'joined_office',
    'syncing',
    'degraded',
    'disconnecting',
    'stopping',
  ].includes(runtime.lifecycle);
}

export function isRuntimeTransportConnected(runtime: ComputerRuntimeSnapshot): boolean {
  return runtime.lifecycle === 'connected' || runtime.lifecycle === 'joined_office';
}

export type RuntimeExecutionStatus = ComputerRuntimeUserState;

export interface RuntimeProjection {
  status: RuntimeExecutionStatus;
  running: boolean;
  businessConnected: boolean;
  mcpServerCount: number;
  activeMcpServerCount: number;
  stoppedMcpServerCount: number;
  toolCount: number;
}

/**
 * Projects SDK-owned runtime state without taking ownership of client connection state.
 * A transport connection is not business-usable until the SDK has joined the Office, and
 * runtime events may demote but never promote the client-owned connection snapshot.
 */
export function projectRuntimeSnapshot(
  runtime: ComputerRuntimeSnapshot,
  clientConnected: boolean,
): RuntimeProjection {
  const running = isRuntimeRunning(runtime);
  return {
    status: runtime.user_state,
    running,
    businessConnected: clientConnected && runtime.lifecycle === 'joined_office',
    mcpServerCount: runtime.mcp_servers,
    activeMcpServerCount: runtime.active_mcp_servers,
    stoppedMcpServerCount: Math.max(0, runtime.mcp_servers - runtime.active_mcp_servers),
    toolCount: runtime.tools,
  };
}

export function shouldAcceptRuntimeSnapshot(
  current: ComputerRuntimeSnapshot | undefined,
  incoming: ComputerRuntimeSnapshot,
): boolean {
  if (!current) return true;
  if (incoming.incarnation !== current.incarnation) {
    return incoming.incarnation > current.incarnation;
  }
  if (incoming.generation !== current.generation) {
    return incoming.generation > current.generation;
  }
  if (incoming.snapshot_revision !== current.snapshot_revision) {
    return incoming.snapshot_revision > current.snapshot_revision;
  }
  return false;
}

export function preferRuntimeSnapshot(
  current: ComputerRuntimeSnapshot | undefined,
  incoming: ComputerRuntimeSnapshot,
): ComputerRuntimeSnapshot {
  if (!current) return incoming;
  if (incoming.incarnation !== current.incarnation) {
    return incoming.incarnation > current.incarnation ? incoming : current;
  }
  if (incoming.generation !== current.generation) {
    return incoming.generation > current.generation ? incoming : current;
  }
  if (incoming.snapshot_revision !== current.snapshot_revision) {
    return incoming.snapshot_revision > current.snapshot_revision ? incoming : current;
  }
  return current;
}

export function preferRuntimeEventSnapshot(
  current: ComputerRuntimeSnapshot | undefined,
  incoming: ComputerRuntimeSnapshot,
): ComputerRuntimeSnapshot {
  return shouldAcceptRuntimeSnapshot(current, incoming) ? incoming : current!;
}

export function acceptAuthoritativeRuntimeSnapshot(
  instanceId: string,
  incoming: ComputerRuntimeSnapshot,
  options?: { allowDeletedRediscovery?: boolean },
): { accepted: boolean; previous: ComputerRuntimeSnapshot | undefined } {
  const previous = authoritativeSnapshots.get(instanceId);
  const deletedIncarnation = deletedIncarnations.get(instanceId);
  if (deletedIncarnation !== undefined) {
    if (
      !options?.allowDeletedRediscovery
      || incoming.incarnation <= deletedIncarnation
    ) {
      return { accepted: false, previous };
    }
    // Explicit deletion is a durable event fence. Only a trusted status response issued after
    // deletion (for example create/list reconciliation) may admit a newly created incarnation.
    deletedIncarnations.delete(instanceId);
  }
  const retiredIncarnation = retiredIncarnations.get(instanceId);
  if (retiredIncarnation !== undefined && incoming.incarnation <= retiredIncarnation) {
    return { accepted: false, previous };
  }
  if (!shouldAcceptRuntimeSnapshot(previous, incoming)) {
    return { accepted: false, previous };
  }
  retiredIncarnations.delete(instanceId);
  authoritativeSnapshots.set(instanceId, incoming);
  return { accepted: true, previous };
}

export function getAuthoritativeRuntimeSnapshot(
  instanceId: string,
): ComputerRuntimeSnapshot | undefined {
  return authoritativeSnapshots.get(instanceId);
}

export function resolveRuntimeSnapshot(
  instanceId: string,
  current: ComputerRuntimeSnapshot | undefined,
  incoming: ComputerRuntimeSnapshot,
  eventSnapshot = false,
): ComputerRuntimeSnapshot {
  let runtime = eventSnapshot
    ? preferRuntimeEventSnapshot(current, incoming)
    : preferRuntimeSnapshot(current, incoming);
  const authoritative = getAuthoritativeRuntimeSnapshot(instanceId);
  if (authoritative) runtime = preferRuntimeEventSnapshot(runtime, authoritative);
  return runtime;
}

export function clearAuthoritativeRuntimeSnapshots() {
  authoritativeSnapshots.clear();
  deletedIncarnations.clear();
  retiredIncarnations.clear();
}

export function evictAuthoritativeRuntimeSnapshot(
  instanceId: string,
  incarnation: number,
) {
  const currentIncarnation = authoritativeSnapshots.get(instanceId)?.incarnation ?? 0;
  const retiredIncarnation = retiredIncarnations.get(instanceId) ?? 0;
  deletedIncarnations.set(
    instanceId,
    Math.max(
      deletedIncarnations.get(instanceId) ?? 0,
      currentIncarnation,
      retiredIncarnation,
      incarnation,
    ),
  );
  authoritativeSnapshots.delete(instanceId);
  retiredIncarnations.delete(instanceId);
}

export function forgetAuthoritativeRuntimeSnapshot(instanceId: string) {
  const current = authoritativeSnapshots.get(instanceId);
  if (current) {
    retiredIncarnations.set(
      instanceId,
      Math.max(retiredIncarnations.get(instanceId) ?? 0, current.incarnation),
    );
  }
  authoritativeSnapshots.delete(instanceId);
}

export function isRuntimeSnapshotInstanceDeleted(instanceId: string): boolean {
  return deletedIncarnations.has(instanceId);
}

export function canProjectRuntimeIncarnation(
  instanceId: string,
  incarnation: number,
): boolean {
  const deletedIncarnation = deletedIncarnations.get(instanceId);
  // Unlike a retired runtime handle, an explicitly deleted instance must not be revived by any
  // queued event, even if that event carries an incarnation unknown to the UI at deletion time.
  if (deletedIncarnation !== undefined) return false;
  const retiredIncarnation = retiredIncarnations.get(instanceId);
  if (retiredIncarnation !== undefined && incarnation <= retiredIncarnation) return false;
  const currentIncarnation = authoritativeSnapshots.get(instanceId)?.incarnation;
  return currentIncarnation === undefined || incarnation >= currentIncarnation;
}
