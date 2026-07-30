import type {
  ComputerConnectionTarget,
  ComputerInstance,
  ComputerStatus,
} from '@/stores/computerStore';
import type {
  ClientConnectionOperation,
  ClientConnectionOperationTarget,
} from '@/stores/connectionAuthority';

export const computerStatusColor: Record<ComputerStatus, string> = {
  running: 'success',
  not_running: 'default',
  starting: 'processing',
  stopping: 'processing',
  degraded: 'warning',
  error: 'error',
};

export function usesStopAction(status: ComputerStatus): boolean {
  return status === 'running' || status === 'degraded' || status === 'stopping';
}

export function isConnectionTargetConnectable(
  target?: ComputerConnectionTarget | null,
): boolean {
  if (!target) return false;
  if (target.type === 'manager_robot') return target.robotAccountId != null;
  return true;
}

export function resolveComputerConnection(instance: ComputerInstance) {
  const authority = instance.connectionState;
  const policyTarget = instance.connectionPolicy.target;
  const actions = authority?.actions ?? {
    connect: instance.runtime.actions.connect,
    disconnect: instance.runtime.actions.disconnect,
  };
  const status = authority?.status
    ?? (instance.clientConnectionPresent ? 'connected' : instance.connectionStatus);
  const operation = authority?.operation ?? null;
  const operationTarget = operation ? authority?.operation_target ?? null : null;
  const targetSelected = operationTarget != null || Boolean(policyTarget);
  const targetConnectable = operationTarget != null
    || isConnectionTargetConnectable(policyTarget);
  const isConnecting = operation === 'connect'
    || operation === 'reconnect'
    || (operation == null && status === 'connecting');
  const isDisconnecting = operation === 'disconnect'
    || (operation == null && status === 'disconnecting');
  const showDisconnect = isDisconnecting
    || (operation == null && actions.disconnect.enabled);

  return {
    actions,
    status,
    operation,
    operationTarget,
    policyTarget,
    targetSelected,
    targetConnectable,
    canConnect: operation == null && actions.connect.enabled && targetConnectable,
    showDisconnect,
    isConnecting,
    isDisconnecting,
  };
}

export type ResolvedComputerConnection = ReturnType<typeof resolveComputerConnection>;

export function connectDisabledReasonTranslationKey(
  connection: ResolvedComputerConnection,
): string | undefined {
  if (!connection.actions.connect.enabled) {
    return connection.actions.connect.disabled_reason
      ? `computer.connectionActions.disabledReasons.${connection.actions.connect.disabled_reason}`
      : undefined;
  }
  if (!connection.targetSelected) return 'computer.connectionActions.requiresTarget';
  if (!connection.targetConnectable) {
    return 'computer.connectionActions.missingRobotAccountId';
  }
  return undefined;
}

export function connectionOperationTargetLabel(
  operation: ClientConnectionOperation,
  target: ClientConnectionOperationTarget,
): string {
  const identifier = target.employee_id != null
    ? String(target.employee_id)
    : target.target_id;
  return identifier
    ? `${target.source_type} · ${identifier}`
    : `${target.source_type} · ${operation}`;
}
