import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  App,
  Button,
  Card,
  Descriptions,
  Empty,
  List,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
} from 'antd';
import {
  ExportOutlined,
  LoginOutlined,
  RobotOutlined,
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
  type DepartmentRef,
  type DigitalEmployeeBrief,
  type ManagerContextKey,
} from '@/stores/managerStore';

const { Text } = Typography;
const EMPTY_MANAGER_EMPLOYEES: DigitalEmployeeBrief[] = [];

interface RobotConnectionPanelProps {
  instanceId: string;
  onNavigate?: (key: string) => void;
}

function formatDeptBreadcrumb(dept: DepartmentRef): string {
  const chain = dept.ancestors?.length ? dept.ancestors.map((ancestor) => ancestor.name) : [dept.name];
  return chain.join(' / ');
}

function employeeStatusTagColor(status?: string): string {
  switch (status) {
    case 'running':
      return 'green';
    case 'suspended':
      return 'orange';
    case 'stopped':
      return 'default';
    case 'init_failed':
    case 'failed':
      return 'red';
    default:
      return 'blue';
  }
}

export function RobotConnectionPanel({ instanceId, onNavigate }: RobotConnectionPanelProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [savingPolicy, setSavingPolicy] = useState(false);
  const { instances, updateConnectionPolicy } = useComputerStore();
  const {
    context,
    employeeResources,
    identityLoading,
    identityError,
    fetchEmployeesIfStale,
  } = useManagerStore();
  const session = managerSessionFromContext(context);
  const authenticatedScope = managerContextScope(context);
  const managerResource = currentEmployeeResource({ context, employeeResources });
  const employees = managerResource?.employees ?? EMPTY_MANAGER_EMPLOYEES;
  const managerLoading = identityLoading || managerResource?.loading === true;
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
    setSavingPolicy(true);
    try {
      await updateConnectionPolicy(instanceId, {
        target,
        auto_connect: nextAutoConnect,
      });
      if (options.showSuccess !== false) {
        message.success(t('common.saved'));
      }
    } catch (e) {
      rollbackPolicyControls();
      message.error(String(e));
    } finally {
      setSavingPolicy(false);
    }
  }, [instanceId, message, rollbackPolicyControls, t, updateConnectionPolicy]);

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
      <Alert
        type="info"
        showIcon
        message={t('computer.connectionActions.readOnlyTitle')}
        description={t('computer.connectionActions.readOnlyDescription')}
        action={
          <Button
            size="small"
            icon={<ExportOutlined />}
            onClick={() => onNavigate?.('robot-connections')}
          >
            {t('computer.connectionActions.openRobotConnections')}
          </Button>
        }
      />

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

      {session ? (
        <Card
          title={t('managerAccount.employees.title')}
          extra={
            <Button onClick={() => fetchEmployeesIfStale(0)} loading={managerLoading}>
              {t('common.refresh')}
            </Button>
          }
        >
          <Space direction="vertical" size={12} style={{ width: '100%' }}>
            {managerError && (
              <Alert type="error" showIcon message={t(`manager.errors.${managerError.kind}`)} />
            )}
            {!managerLoading && employees.length === 0 && !managerError ? (
              <Empty description={t('managerAccount.employees.empty')} />
            ) : (
              <List
                bordered
                loading={managerLoading}
                dataSource={employees}
                rowKey={(employee) => employee.id}
                renderItem={(employee) => (
                  <List.Item>
                    <List.Item.Meta
                      avatar={<RobotOutlined style={{ fontSize: 24 }} />}
                      title={
                        <Space size={4} wrap>
                          <Text strong>{employee.name}</Text>
                          {employee.status && (
                            <Tag color={employeeStatusTagColor(employee.status)}>
                              {employee.status}
                            </Tag>
                          )}
                          {employee.templateDisplayName && (
                            <Tag>{employee.templateDisplayName}</Tag>
                          )}
                          {employee.templateType && (
                            <Tag color="purple">{employee.templateType}</Tag>
                          )}
                        </Space>
                      }
                      description={
                        <Descriptions size="small" column={1} colon={false}>
                          <Descriptions.Item label={t('managerAccount.employees.department')}>
                            {employee.departments && employee.departments.length > 0 ? (
                              <Space direction="vertical" size={0}>
                                {employee.departments.map((dept) => (
                                  <Text key={dept.id}>{formatDeptBreadcrumb(dept)}</Text>
                                ))}
                              </Space>
                            ) : (
                              <Text type="secondary">
                                {t('managerAccount.employees.noDepartment')}
                              </Text>
                            )}
                          </Descriptions.Item>
                          {employee.robotId && (
                            <Descriptions.Item
                              label={t('managerAccount.employees.robotId')}
                              contentStyle={{ fontFamily: 'monospace' }}
                            >
                              {employee.robotId}
                            </Descriptions.Item>
                          )}
                          {employee.namespace && (
                            <Descriptions.Item label={t('managerAccount.employees.namespace')}>
                              {employee.namespace}
                            </Descriptions.Item>
                          )}
                          {employee.clusterName && (
                            <Descriptions.Item label={t('managerAccount.employees.cluster')}>
                              {employee.clusterName}
                            </Descriptions.Item>
                          )}
                          {employee.description && (
                            <Descriptions.Item label={t('managerAccount.employees.description')}>
                              {employee.description}
                            </Descriptions.Item>
                          )}
                        </Descriptions>
                      }
                    />
                  </List.Item>
                )}
              />
            )}
          </Space>
        </Card>
      ) : (
        <Card>
          <Empty description={t('computer.connectionActions.managerLoginRequired')}>
            <Space>
              <LoginOutlined />
              <Text type="secondary">
                {t('computer.connectionActions.useGlobalManagerAccount')}
              </Text>
            </Space>
          </Empty>
        </Card>
      )}
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
