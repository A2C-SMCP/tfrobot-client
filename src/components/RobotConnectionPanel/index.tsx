import { ConfigSaveStatus } from '@/components/ComputerSettings/ConfigSaveStatus';
import { useConfigCommit } from '@/components/ComputerSettings/useConfigCommit';
import { usePageAction } from '@/components/Navigation/pageActivityState';
import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  App,
  Button,
  Card,
  Empty,
  Select,
  Space,
  Switch,
  Typography,
} from 'antd';
import {
  LoginOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  useComputerStore,
  type ComputerConnectionTarget,
} from '@/stores/computerStore';
import {
  currentEmployeeResource,
  managerContextScope,
  managerSessionFromContext,
  useManagerStore,
  type DigitalEmployeeBrief,
  type ManagerContextKey,
} from '@/stores/managerStore';

const { Text } = Typography;
const EMPTY_MANAGER_EMPLOYEES: DigitalEmployeeBrief[] = [];

interface RobotConnectionPanelProps {
  instanceId: string;
}

export function RobotConnectionPanel({ instanceId }: RobotConnectionPanelProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { instances, updateConnectionPolicy } = useComputerStore();
  const {
    context,
    employeeResources,
    identityError,
    fetchEmployeesIfStale,
  } = useManagerStore();
  const session = managerSessionFromContext(context);
  const authenticatedScope = managerContextScope(context);
  const action = usePageAction(`${instanceId}:${authenticatedScope}`);
  const submission = useConfigCommit(`settings.connection.${instanceId}:${authenticatedScope}`, (policy: { target: ComputerConnectionTarget | null; autoConnect: boolean }) => updateConnectionPolicy(instanceId, { target: policy.target, auto_connect: policy.autoConnect }));
  const savingPolicy = submission.state === 'saving';
  const failedPolicy = submission.state === 'error' ? submission.last : null;
  const { commit: commitPolicy, getError: policyError } = submission;
  const managerResource = currentEmployeeResource({ context, employeeResources });
  const employees = managerResource?.employees ?? EMPTY_MANAGER_EMPLOYEES;
  const managerError = managerResource?.error ?? identityError;
  const employeeListLoaded = managerResource?.lastFetchAt !== null
    && managerResource?.lastFetchAt !== undefined;
  const [selectedTargetValue, setSelectedTargetValue] = useState<string>();
  const [autoConnect, setAutoConnect] = useState(false);
  const selectedInstance = instances.find((instance) => instance.id === instanceId);

  useEffect(() => {
    if (authenticatedScope) {
      fetchEmployeesIfStale().catch(() => {
        /* error is rendered from manager store */
      });
    }
  }, [authenticatedScope, fetchEmployeesIfStale]);

  useEffect(() => {
    setSelectedTargetValue(managerTargetToValue(
      selectedInstance?.connectionPolicy.target,
      context.contextKey,
    ));
    setAutoConnect(selectedInstance?.connectionPolicy.auto_connect ?? false);
  }, [
    selectedInstance?.connectionPolicy.auto_connect,
    selectedInstance?.connectionPolicy.target,
    context.contextKey,
  ]);

  const selectedTarget = useMemo(
    () => valueToTarget(
      selectedTargetValue,
      employees,
      selectedInstance?.connectionPolicy.target,
      context.contextKey,
    ),
    [context.contextKey, employees, selectedInstance?.connectionPolicy.target, selectedTargetValue],
  );
  const bindingPresentation = managerBindingPresentation(
    selectedInstance?.connectionPolicy.target,
    selectedInstance?.robotBinding,
    context.contextKey,
    employees,
    employeeListLoaded,
  );
  const targetOptions = useMemo(() => {
    if (!context.contextKey) return [];
    const managerOptions = employees
      .filter((employee) => (employee.status ?? 'running') === 'running')
      .map((employee) => ({
        value: targetToValue({
          type: 'manager_robot',
          contextKey: context.contextKey!,
          employeeId: employee.id,
          lastResolvedRobotAccountId: employee.robotAccountId,
        })!,
        label: formatManagerRobotOption(employee),
        disabled: false,
      }));
    const persistedTarget = selectedInstance?.connectionPolicy.target;
    if (
      persistedTarget?.type === 'manager_robot'
      && sameContextKey(persistedTarget.contextKey, context.contextKey)
      && !employees.some((employee) => employee.id === persistedTarget.employeeId)
    ) {
      managerOptions.push({
        value: targetToValue(persistedTarget)!,
        label: selectedInstance?.robotBinding?.robot_name
          ?? t('computer.connectionActions.savedManagerRobot', {
            employeeId: persistedTarget.employeeId,
          }),
        disabled: true,
      });
    }
    return [
      {
        label: t('managerAccount.employees.managerRobots'),
        options: managerOptions,
      },
    ];
  }, [context.contextKey, employees, selectedInstance, t]);

  const rollbackPolicyControls = useCallback(() => {
    setSelectedTargetValue(managerTargetToValue(
      selectedInstance?.connectionPolicy.target,
      context.contextKey,
    ));
    setAutoConnect(selectedInstance?.connectionPolicy.auto_connect ?? false);
  }, [
    selectedInstance?.connectionPolicy.auto_connect,
    selectedInstance?.connectionPolicy.target,
    context.contextKey,
  ]);

  const savePolicy = useCallback(async (
    target: ComputerConnectionTarget | null,
    nextAutoConnect: boolean,
    options: { showSuccess?: boolean } = {},
  ) => {
    const current = action();
    const acknowledged = await commitPolicy({ target, autoConnect: nextAutoConnect });
    if (!current()) return;
    if (acknowledged && options.showSuccess !== false) message.success(t('common.saved'));
    else if (policyError()) {
      rollbackPolicyControls();
      message.error(String(policyError()));
    }
  }, [action, commitPolicy, policyError, message, rollbackPolicyControls, t]);

  const handleTargetChange = (value: string | undefined) => {
    const nextTarget = valueToTarget(
      value,
      employees,
      selectedInstance?.connectionPolicy.target,
      context.contextKey,
    );
    setSelectedTargetValue(value);
    void savePolicy(nextTarget, autoConnect);
  };

  const handleAutoConnectChange = (checked: boolean) => {
    setAutoConnect(checked);
    void savePolicy(selectedTarget, checked);
  };

  const handleReactivate = () => {
    if (!selectedTarget) return;
    void savePolicy(selectedTarget, false);
  };

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small" title={t('computer.connectionActions.targetTitle')}>
        <Space direction="vertical" size={12} style={{ width: '100%' }}>
          {bindingPresentation !== 'active' && bindingPresentation !== 'unbound' && (
            <Alert
              type={bindingPresentation === 'permission_revoked' ? 'error' : 'warning'}
              showIcon
              message={t(`computer.connectionActions.binding.${bindingPresentation}.title`)}
              description={t(
                `computer.connectionActions.binding.${bindingPresentation}.description`,
              )}
              action={bindingPresentation === 'dormant' && selectedTarget ? (
                <Button
                  size="small"
                  disabled={savingPolicy || !session}
                  onClick={handleReactivate}
                >
                  {t('computer.connectionActions.binding.reactivate')}
                </Button>
              ) : undefined}
            />
          )}
          <ConfigSaveStatus state={submission.state} onRetry={() => failedPolicy && savePolicy(failedPolicy.target, failedPolicy.autoConnect)} />
          <Text type="secondary">{t('computer.adoption.connectionPolicyHint')}</Text>
          <Space wrap>
          <Select
            allowClear
            disabled={savingPolicy || !session}
            style={{ width: 'min(480px, 100%)', minWidth: 240 }}
            value={selectedTargetValue}
            placeholder={t('computer.connectionActions.selectTarget')}
            aria-label={t('computer.connectionActions.selectTarget')}
            onChange={handleTargetChange}
            options={targetOptions}
          />
          <Button
            disabled={savingPolicy || !session}
            onClick={() => session && fetchEmployeesIfStale(0)}
          >
            {t('common.refresh')}
          </Button>
          <Space>
            <Switch
              checked={autoConnect}
              disabled={!selectedTarget || bindingPresentation !== 'active'}
              loading={savingPolicy}
              aria-label={t('connection.form.autoConnect')}
              onChange={handleAutoConnectChange}
            />
            <Text>{t('connection.form.autoConnect')}</Text>
          </Space>
          </Space>
        </Space>
      </Card>

      {managerError && <Alert type="error" showIcon message={t(`manager.errors.${managerError.kind}`)} />}
      {!session && <Empty description={t('computer.connectionActions.managerLoginRequired')}>
        <Space><LoginOutlined /><Text type="secondary">{t('computer.connectionActions.useGlobalManagerAccount')}</Text></Space>
      </Empty>}

    </Space>
  );
}

function targetToValue(target?: ComputerConnectionTarget | null): string | undefined {
  if (!target) return undefined;
  if (target.type === 'manual_smcp') return `${target.type}:${target.id}`;
  return JSON.stringify([
    target.type,
    target.contextKey.environment,
    target.contextKey.accountId,
    target.contextKey.organizationId,
    target.employeeId,
  ]);
}

function managerTargetToValue(
  target: ComputerConnectionTarget | null | undefined,
  currentContextKey: ManagerContextKey | null,
): string | undefined {
  return target?.type === 'manager_robot'
    && currentContextKey !== null
    && sameContextKey(target.contextKey, currentContextKey)
    ? targetToValue(target)
    : undefined;
}

function formatManagerRobotOption(employee: {
  name: string;
  status?: string;
  templateDisplayName?: string;
  templateType?: string;
  namespace?: string;
}): string {
  const parts = [
    employee.status,
    employee.templateDisplayName,
    employee.templateType,
    employee.namespace,
  ].filter(Boolean);
  return parts.length > 0 ? `${employee.name} (${parts.join(' / ')})` : employee.name;
}

function valueToTarget(
  value: string | undefined,
  employees: Array<{ id: number; robotAccountId?: string }>,
  currentTarget?: ComputerConnectionTarget | null,
  currentContextKey?: ManagerContextKey | null,
): ComputerConnectionTarget | null {
  if (!value || !currentContextKey) return null;
  if (currentTarget?.type === 'manager_robot' && targetToValue(currentTarget) === value) {
    const employee = employees.find((item) => item.id === currentTarget.employeeId);
    return {
      ...currentTarget,
      lastResolvedRobotAccountId:
        employee?.robotAccountId ?? currentTarget.lastResolvedRobotAccountId,
    };
  }
  const employee = employees.find((item) => targetToValue({
    type: 'manager_robot',
    contextKey: currentContextKey,
    employeeId: item.id,
    lastResolvedRobotAccountId: item.robotAccountId,
  }) === value);
  if (!employee) return null;
  return {
    type: 'manager_robot',
    contextKey: currentContextKey,
    employeeId: employee.id,
    lastResolvedRobotAccountId: employee.robotAccountId,
  };
}

function sameContextKey(left: ManagerContextKey, right: ManagerContextKey): boolean {
  return left.environment === right.environment
    && left.accountId === right.accountId
    && left.organizationId === right.organizationId;
}

type ManagerBindingPresentation =
  | 'active'
  | 'dormant'
  | 'needs_rebind'
  | 'permission_revoked'
  | 'unbound';

function managerBindingPresentation(
  target: ComputerConnectionTarget | null | undefined,
  binding: {
    context_key?: ManagerContextKey;
    state: 'active' | 'dormant' | 'needs_rebind';
    employee_id: number;
  } | null | undefined,
  currentContext: ManagerContextKey | null,
  employees: DigitalEmployeeBrief[],
  employeeListLoaded: boolean,
): ManagerBindingPresentation {
  if (binding?.state === 'needs_rebind') return 'needs_rebind';
  const managerTarget = target?.type === 'manager_robot' ? target : null;
  if (!managerTarget && !binding) return 'unbound';
  if (
    currentContext === null
    || managerTarget === null
    || !sameContextKey(managerTarget.contextKey, currentContext)
    || (binding?.context_key !== undefined
      && !sameContextKey(binding.context_key, currentContext))
    || binding?.state === 'dormant'
  ) {
    return 'dormant';
  }
  if (
    employeeListLoaded
    && !employees.some((employee) => employee.id === managerTarget.employeeId)
  ) {
    return 'permission_revoked';
  }
  return 'active';
}
