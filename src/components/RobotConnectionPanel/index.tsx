import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
import { useManagerStore, type DepartmentRef } from '@/stores/managerStore';

const { Text } = Typography;

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
  const hydrationAttemptedFor = useRef<string | null>(null);
  const { instances, updateConnectionPolicy } = useComputerStore();
  const {
    session,
    employees,
    loading: managerLoading,
    error: managerError,
    fetchEmployeesIfStale,
  } = useManagerStore();
  const [selectedTargetValue, setSelectedTargetValue] = useState<string>();
  const [autoConnect, setAutoConnect] = useState(false);
  const selectedInstance = instances.find((instance) => instance.id === instanceId);

  useEffect(() => {
    if (session) {
      fetchEmployeesIfStale().catch(() => {
        /* error is rendered from manager store */
      });
    }
  }, [fetchEmployeesIfStale, session]);

  useEffect(() => {
    setSelectedTargetValue(managerTargetToValue(selectedInstance?.connectionPolicy.target));
    setAutoConnect(selectedInstance?.connectionPolicy.auto_connect ?? false);
  }, [
    selectedInstance?.connectionPolicy.auto_connect,
    selectedInstance?.connectionPolicy.target,
  ]);

  const selectedTarget = useMemo(
    () => valueToTarget(
      selectedTargetValue,
      employees,
      selectedInstance?.connectionPolicy.target,
    ),
    [employees, selectedInstance?.connectionPolicy.target, selectedTargetValue],
  );
  const targetOptions = useMemo(() => {
    const managerOptions = employees
      .filter((employee) => (employee.status ?? 'running') === 'running' && employee.robotAccountId != null)
      .map((employee) => ({
        value: targetToValue({
          type: 'manager_robot',
          id: String(employee.id),
          robotAccountId: employee.robotAccountId,
        })!,
        label: formatManagerRobotOption(employee),
      }));
    return [
      {
        label: t('managerAccount.employees.managerRobots'),
        options: managerOptions,
      },
    ];
  }, [employees, t]);

  const rollbackPolicyControls = useCallback(() => {
    setSelectedTargetValue(managerTargetToValue(selectedInstance?.connectionPolicy.target));
    setAutoConnect(selectedInstance?.connectionPolicy.auto_connect ?? false);
  }, [
    selectedInstance?.connectionPolicy.auto_connect,
    selectedInstance?.connectionPolicy.target,
  ]);

  const savePolicy = useCallback(async (
    target: ComputerConnectionTarget | null,
    nextAutoConnect: boolean,
    options: { showSuccess?: boolean } = {},
  ) => {
    if (target?.type === 'manager_robot' && target.robotAccountId == null) {
      message.error(t('managerAccount.employees.noRobotAccount'));
      rollbackPolicyControls();
      return;
    }
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
    );
    setSelectedTargetValue(value);
    void savePolicy(nextTarget, autoConnect);
  };

  const handleAutoConnectChange = (checked: boolean) => {
    setAutoConnect(checked);
    void savePolicy(selectedTarget, checked);
  };

  useEffect(() => {
    if (
      savingPolicy
      || selectedTarget?.type !== 'manager_robot'
      || selectedTarget.robotAccountId == null
      || selectedInstance?.connectionPolicy.target?.type !== 'manager_robot'
      || selectedInstance.connectionPolicy.target.robotAccountId != null
      || targetToValue(selectedInstance.connectionPolicy.target) !== targetToValue(selectedTarget)
    ) {
      return;
    }
    const targetValue = targetToValue(selectedTarget);
    if (hydrationAttemptedFor.current === targetValue) {
      return;
    }
    hydrationAttemptedFor.current = targetValue ?? null;
    void savePolicy(selectedTarget, autoConnect, { showSuccess: false });
  }, [
    autoConnect,
    savePolicy,
    savingPolicy,
    selectedInstance?.connectionPolicy.target,
    selectedTarget,
  ]);

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
        <Space wrap>
          <Select
            allowClear
            disabled={savingPolicy}
            style={{ minWidth: 360 }}
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
              disabled={!selectedTarget}
              loading={savingPolicy}
              aria-label={t('connection.form.autoConnect')}
              onChange={handleAutoConnectChange}
            />
            <Text>{t('connection.form.autoConnect')}</Text>
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
            <Button
              type="primary"
              icon={<LoginOutlined />}
              onClick={() => onNavigate?.('robot-connections')}
            >
              {t('computer.connectionActions.openRobotConnections')}
            </Button>
          </Empty>
        </Card>
      )}
    </Space>
  );
}

function targetToValue(target?: ComputerConnectionTarget | null): string | undefined {
  if (!target) return undefined;
  return `${target.type}:${target.id}`;
}

function managerTargetToValue(target?: ComputerConnectionTarget | null): string | undefined {
  return target?.type === 'manager_robot' ? targetToValue(target) : undefined;
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
): ComputerConnectionTarget | null {
  if (!value) return null;
  const [type, ...idParts] = value.split(':');
  const id = idParts.join(':');
  if (!id || type !== 'manager_robot') return null;
  const employee = employees.find((item) => String(item.id) === id);
  if (currentTarget?.type === 'manager_robot' && targetToValue(currentTarget) === value) {
    return {
      ...currentTarget,
      robotAccountId: employee?.robotAccountId ?? currentTarget.robotAccountId,
    };
  }
  return {
    type,
    id,
    robotAccountId: employee?.robotAccountId,
  };
}
