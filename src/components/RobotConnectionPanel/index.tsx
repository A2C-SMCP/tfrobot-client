import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Alert,
  App,
  Button,
  Card,
  Descriptions,
  Empty,
  List,
  Modal,
  Select,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd';
import {
  ApiOutlined,
  ExportOutlined,
  InfoCircleOutlined,
  LoginOutlined,
  RobotOutlined,
  UserOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  useComputerStore,
  type ComputerConnectionTarget,
  type ComputerConnectionTargetType,
} from '@/stores/computerStore';
import { useConnectionTargetStore, type ManualSmcpTarget } from '@/stores/connectionTargetStore';
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
  const [detailTarget, setDetailTarget] = useState<ManualSmcpTarget | null>(null);
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
  const {
    manualTargets,
    loading: targetLoading,
    error: targetError,
    fetchManualTargets,
  } = useConnectionTargetStore();
  const [selectedTargetValue, setSelectedTargetValue] = useState<string>();
  const [autoConnect, setAutoConnect] = useState(false);
  const selectedInstance = instances.find((instance) => instance.id === instanceId);

  useEffect(() => {
    fetchManualTargets();
    if (session) {
      fetchEmployeesIfStale().catch(() => {
        /* error is rendered from manager store */
      });
    }
  }, [fetchManualTargets, fetchEmployeesIfStale, session]);

  useEffect(() => {
    setSelectedTargetValue(targetToValue(selectedInstance?.connectionPolicy.target));
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
    const manualOptions = manualTargets.map((target) => ({
      value: targetToValue({ type: 'manual_smcp', id: target.id })!,
      label: `${target.name} (${target.office_id})`,
    }));
    return [
      {
        label: t('managerAccount.employees.managerRobots'),
        options: managerOptions,
      },
      {
        label: t('connection.manualSmcp'),
        options: manualOptions,
      },
    ];
  }, [employees, manualTargets, t]);

  const manualColumns = useMemo(
    () => [
      {
        title: t('connection.table.name'),
        dataIndex: 'name',
        key: 'name',
        render: (name: string) => <Text strong>{name}</Text>,
      },
      { title: t('connection.table.url'), dataIndex: 'url', key: 'url', ellipsis: true },
      { title: t('connection.table.office'), dataIndex: 'office_id', key: 'office_id' },
      {
        title: t('connection.table.actions'),
        key: 'actions',
        width: 120,
        render: (_: unknown, record: ManualSmcpTarget) => (
          <Button
            type="link"
            icon={<InfoCircleOutlined />}
            onClick={() => setDetailTarget(record)}
          >
            {t('computer.connectionActions.openTargetDetails')}
          </Button>
        ),
      },
    ],
    [t],
  );

  const rollbackPolicyControls = useCallback(() => {
    setSelectedTargetValue(targetToValue(selectedInstance?.connectionPolicy.target));
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
            onChange={handleTargetChange}
            options={targetOptions}
          />
          <Button disabled={savingPolicy} onClick={() => {
            fetchManualTargets();
            if (session) fetchEmployeesIfStale(0);
          }}>
            {t('common.refresh')}
          </Button>
          <Space>
            <Switch checked={autoConnect} loading={savingPolicy} onChange={handleAutoConnectChange} />
            <Text>{t('connection.form.autoConnect')}</Text>
          </Space>
        </Space>
      </Card>

      <Tabs
        items={[
          {
            key: 'manager',
            label: (
              <>
                <UserOutlined /> {t('managerAccount.employees.managerRobots')}
              </>
            ),
            children: session ? (
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
                    <Alert
                      type="error"
                      showIcon
                      message={t(`manager.errors.${managerError.kind}`)}
                    />
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
                <Empty
                  description={t('computer.connectionActions.managerLoginRequired')}
                >
                  <Button
                    type="primary"
                    icon={<LoginOutlined />}
                    onClick={() => onNavigate?.('robot-connections')}
                  >
                    {t('computer.connectionActions.openRobotConnections')}
                  </Button>
                </Empty>
              </Card>
            ),
          },
          {
            key: 'manual',
            label: (
              <>
                <ApiOutlined /> {t('connection.manualSmcp')}
              </>
            ),
            children: (
              <Card
                title={t('computer.connectionActions.manualTargetsTitle')}
                extra={
                  <Space>
                    <Button onClick={() => fetchManualTargets()} loading={targetLoading}>
                      {t('common.refresh')}
                    </Button>
                    <Button
                      icon={<ExportOutlined />}
                      onClick={() => onNavigate?.('robot-connections')}
                    >
                      {t('computer.connectionActions.configureTargets')}
                    </Button>
                  </Space>
                }
              >
                <Space direction="vertical" size={12} style={{ width: '100%' }}>
                  {targetError && <Alert type="error" showIcon message={targetError} />}
                  <Table
                    rowKey="id"
                    columns={manualColumns}
                    dataSource={manualTargets}
                    loading={targetLoading}
                    pagination={false}
                    locale={{ emptyText: t('computer.connectionActions.noManualTargets') }}
                  />
                </Space>
              </Card>
            ),
          },
        ]}
      />

      <Modal
        open={Boolean(detailTarget)}
        title={detailTarget?.name}
        footer={null}
        onCancel={() => setDetailTarget(null)}
        destroyOnHidden
      >
        {detailTarget && (
          <Descriptions size="small" column={1} bordered>
            <Descriptions.Item label={t('connection.table.name')}>
              {detailTarget.name}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.table.url')}>
              {detailTarget.url}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.table.office')}>
              {detailTarget.office_id}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.form.namespace')}>
              {detailTarget.namespace}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.form.headers')}>
              {Object.keys(detailTarget.headers).length > 0
                ? JSON.stringify(detailTarget.headers)
                : '-'}
            </Descriptions.Item>
          </Descriptions>
        )}
      </Modal>
    </Space>
  );
}

function targetToValue(target?: ComputerConnectionTarget | null): string | undefined {
  if (!target) return undefined;
  return `${target.type}:${target.id}`;
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
  employees: Array<{ id: number; robotAccountId?: number }>,
  currentTarget?: ComputerConnectionTarget | null,
): ComputerConnectionTarget | null {
  if (!value) return null;
  const [type, ...idParts] = value.split(':');
  const id = idParts.join(':');
  if (!id || (type !== 'manager_robot' && type !== 'manual_smcp')) return null;
  if (type === 'manager_robot') {
    const employee = employees.find((item) => String(item.id) === id);
    if (currentTarget && targetToValue(currentTarget) === value) {
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
  if (currentTarget && targetToValue(currentTarget) === value) {
    return currentTarget;
  }
  return { type: type as ComputerConnectionTargetType, id };
}
