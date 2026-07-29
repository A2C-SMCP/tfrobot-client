import type { ComputerRuntimeSnapshot } from './runtimeSnapshot';

export interface ConnectionStateSummary {
  url: string;
  office_id: string;
  computer_name: string;
  connected_at: string;
  profile_name: string;
  source_type?: string;
  target_id?: string | null;
  target_name?: string | null;
  employee_id?: number | null;
}

export type ClientConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'disconnecting';
export type ClientConnectionOperation = 'connect' | 'disconnect' | 'reconnect';
export interface ClientConnectionOperationTarget {
  source_type: string;
  target_id: string | null;
  employee_id: number | null;
}
export type ClientConnectionActionDisabledReason =
  | 'already_connected'
  | 'not_connected'
  | 'transition_in_progress'
  | 'not_running'
  | 'connection_unavailable';

export interface ClientConnectionOperationError {
  operation: ClientConnectionOperation;
  message: string;
  retryable: boolean;
  occurred_at: string;
}

export interface ClientConnectionActionCapability {
  enabled: boolean;
  disabled_reason: ClientConnectionActionDisabledReason | null;
}

export interface ClientConnectionActionCapabilities {
  connect: ClientConnectionActionCapability;
  disconnect: ClientConnectionActionCapability;
}

export interface ClientConnectionAuthority {
  status: ClientConnectionStatus;
  present: boolean;
  context: ConnectionStateSummary | null;
  revision: number;
  operation: ClientConnectionOperation | null;
  operation_target: ClientConnectionOperationTarget | null;
  last_error: ClientConnectionOperationError | null;
  actions: ClientConnectionActionCapabilities;
}

export interface LegacyClientConnectionAuthority {
  present: boolean;
  context: ConnectionStateSummary | null;
  revision: number;
}

export type ClientConnectionAuthorityInput =
  | ClientConnectionAuthority
  | LegacyClientConnectionAuthority;

interface VersionedClientConnectionAuthority extends ClientConnectionAuthority {
  incarnation: ComputerRuntimeSnapshot['incarnation'];
  runtime_generation: ComputerRuntimeSnapshot['generation'];
  runtime_snapshot_revision: ComputerRuntimeSnapshot['snapshot_revision'];
}

const authorities = new Map<string, VersionedClientConnectionAuthority>();

export function setClientConnectionAuthority(
  instanceId: string,
  connection: ClientConnectionAuthorityInput,
  runtime: ComputerRuntimeSnapshot,
): void;
/** @deprecated Compatibility overload for snapshots emitted before TFRC-73. */
export function setClientConnectionAuthority(
  instanceId: string,
  present: boolean,
  context: ConnectionStateSummary | null | undefined,
  revision: number,
  runtime: ComputerRuntimeSnapshot,
): void;
export function setClientConnectionAuthority(
  instanceId: string,
  connectionOrPresent: ClientConnectionAuthorityInput | boolean,
  connectionOrRuntime: ConnectionStateSummary | null | undefined | ComputerRuntimeSnapshot,
  revision?: number,
  legacyRuntime?: ComputerRuntimeSnapshot,
): void {
  const connection = typeof connectionOrPresent === 'boolean'
    ? legacyClientConnectionState(
      connectionOrPresent,
      connectionOrRuntime as ConnectionStateSummary | null | undefined,
      revision ?? 0,
      legacyRuntime,
    )
    : connectionOrPresent;
  const runtime = (typeof connectionOrPresent === 'boolean'
    ? legacyRuntime
    : connectionOrRuntime) as ComputerRuntimeSnapshot;
  const normalized = 'status' in connection
    ? connection
    : legacyClientConnectionState(
      connection.present,
      connection.context,
      connection.revision,
      runtime,
    );
  const current = authorities.get(instanceId);
  if (current && (
    runtime.incarnation < current.incarnation
    || (
      runtime.incarnation === current.incarnation
      && (
        normalized.revision < current.revision
        || (
          normalized.revision === current.revision
          && (
            runtime.generation < current.runtime_generation
            || (
              runtime.generation === current.runtime_generation
              && runtime.snapshot_revision <= current.runtime_snapshot_revision
            )
          )
        )
      )
    )
  )) return;
  authorities.set(instanceId, {
    ...normalized,
    context: normalized.present ? normalized.context ?? null : null,
    operation_target: normalized.operation ? normalized.operation_target ?? null : null,
    incarnation: runtime.incarnation,
    runtime_generation: runtime.generation,
    runtime_snapshot_revision: runtime.snapshot_revision,
  });
}

export function getClientConnectionAuthority(
  instanceId: string,
  incarnation?: number,
): ClientConnectionAuthority | undefined {
  const authority = authorities.get(instanceId);
  if (!authority || (incarnation !== undefined && authority.incarnation !== incarnation)) {
    return undefined;
  }
  return {
    status: authority.status,
    present: authority.present,
    context: authority.context,
    revision: authority.revision,
    operation: authority.operation,
    operation_target: authority.operation_target,
    last_error: authority.last_error,
    actions: authority.actions,
  };
}

export function legacyClientConnectionState(
  present: boolean,
  context: ConnectionStateSummary | null | undefined,
  revision: number,
  runtime?: ComputerRuntimeSnapshot,
): ClientConnectionAuthority {
  const status: ClientConnectionStatus = present ? 'connected' : 'disconnected';
  return {
    status,
    present,
    context: present ? context ?? null : null,
    revision,
    operation: null,
    operation_target: null,
    last_error: null,
    actions: present
      ? {
          connect: { enabled: false, disabled_reason: 'already_connected' },
          disconnect: { enabled: true, disabled_reason: null },
        }
      : runtime
        ? {
            connect: legacyRuntimeCapability(runtime.actions.connect, 'connection_unavailable'),
            disconnect: legacyRuntimeCapability(runtime.actions.disconnect, 'not_connected'),
          }
        : {
            connect: { enabled: true, disabled_reason: null },
            disconnect: { enabled: false, disabled_reason: 'not_connected' },
          },
  };
}

function legacyRuntimeCapability(
  capability: ComputerRuntimeSnapshot['actions']['connect'],
  fallback: ClientConnectionActionDisabledReason,
): ClientConnectionActionCapability {
  if (capability.enabled) return { enabled: true, disabled_reason: null };
  const reason = capability.disabled_reason;
  return {
    enabled: false,
    disabled_reason: reason === 'not_running' || reason === 'transition_in_progress'
      ? reason
      : reason === 'already_running'
        ? 'already_connected'
        : fallback,
  };
}

export function clearClientConnectionAuthority(instanceId: string): void {
  authorities.delete(instanceId);
}

export function resetClientConnectionAuthorities(): void {
  authorities.clear();
}
