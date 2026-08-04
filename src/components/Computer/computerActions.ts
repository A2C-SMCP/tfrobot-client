import type {
  ComputerConnectionTarget,
  ComputerInstance,
  ComputerStatus,
  RobotBindingMetadata,
} from '@/stores/computerStore';
import type { ManagerContextKey } from '@/stores/managerStore';

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
  binding?: RobotBindingMetadata | null,
  currentContext?: ManagerContextKey | null,
): boolean {
  if (!target) return false;
  if (target.type === 'manual_smcp') return true;
  const sameContext = (left?: ManagerContextKey | null, right?: ManagerContextKey | null) =>
    left?.environment === right?.environment
    && left?.accountId === right?.accountId
    && left?.organizationId === right?.organizationId;
  return currentContext != null
    && sameContext(target.contextKey, currentContext)
    && binding?.state === 'active'
    && binding.employee_id === target.employeeId
    && sameContext(binding.context_key, currentContext);
}

export function resolveComputerConnection(
  instance: ComputerInstance,
  currentManagerContext?: ManagerContextKey | null,
) {
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
    || isConnectionTargetConnectable(
      policyTarget,
      instance.robotBinding,
      currentManagerContext,
    );
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
    return 'computer.connectionActions.targetUnavailable';
  }
  return undefined;
}
