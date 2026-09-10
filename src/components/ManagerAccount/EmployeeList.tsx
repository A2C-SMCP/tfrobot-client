import { usePageActive } from '@/components/Navigation/pageActivityState';
import { useEffect } from 'react';
import {
  App,
  Card,
  List,
  Button,
  Typography,
  Space,
  Alert,
  Tag,
  Empty,
  Descriptions,
} from 'antd';
import {
  RobotOutlined,
  ReloadOutlined,
  LogoutOutlined,
  LinkOutlined,
  DisconnectOutlined,
  CheckCircleOutlined,
} from '@ant-design/icons';
import { open as openExternal } from '@tauri-apps/plugin-shell';
import { useTranslation } from 'react-i18next';
import {
  currentEmployeeResource,
  managerContextScope,
  managerSessionFromContext,
  useManagerStore,
  type DepartmentRef,
  type DigitalEmployeeBrief,
  type ManagerError,
} from '@/stores/managerStore';
import { useConnectionStore, type ConnectionStatusInfo } from '@/stores/connectionStore';

const { Title, Text } = Typography;
const EMPTY_EMPLOYEES: DigitalEmployeeBrief[] = [];

const DEFAULT_CONNECT_CAPABILITY = {
  enabled: false,
  disabled_reason: 'connection_unavailable',
} as const;
const DEFAULT_DISCONNECT_CAPABILITY = {
  enabled: false,
  disabled_reason: 'not_connected',
} as const;
const DISCONNECTED_STATUS: ConnectionStatusInfo = {
  status: 'disconnected',
  connected: false,
  actions: {
    connect: DEFAULT_CONNECT_CAPABILITY,
    disconnect: DEFAULT_DISCONNECT_CAPABILITY,
  },
};

interface EmployeeListProps {
  instanceId?: string;
  showIdentityActions?: boolean;
  showIdentitySummary?: boolean;
}

function errorI18nKey(err: ManagerError): string {
  return `manager.errors.${err.kind}`;
}

/** 把单个部门的祖先链拼成面包屑（根→叶有序）。无祖先链时回退到部门名。 */
function formatDeptBreadcrumb(dept: DepartmentRef): string {
  const chain = dept.ancestors?.length ? dept.ancestors.map((a) => a.name) : [dept.name];
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

function isConnectable(emp: DigitalEmployeeBrief): boolean {
  return (emp.status ?? 'running') === 'running';
}

function employeeStatusI18nKey(status: string): string {
  return `managerAccount.employees.status.${status}`;
}

export function EmployeeList({
  instanceId,
  showIdentityActions = true,
  showIdentitySummary = true,
}: EmployeeListProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    context,
    employeeResources,
    identityLoading,
    identityError,
    online,
    fetchEmployees,
    fetchEmployeesIfStale,
    setOnline,
    selectEmployeeAndConnect,
    logout,
    clearError,
    dismissPaymentRequired,
  } = useManagerStore();
  const session = managerSessionFromContext(context);
  const authenticatedScope = managerContextScope(context);
  const resource = currentEmployeeResource({ context, employeeResources });
  const employees = resource?.employees ?? EMPTY_EMPLOYEES;
  const loading = identityLoading
    || resource?.loading === true
    || resource?.connectingEmployeeId != null;
  const error = resource?.error ?? identityError;
  const paymentRequired = resource?.paymentRequired ?? null;
  const disconnectSmcp = useConnectionStore((s) => s.disconnect);
  const selectedConnectionStatus = useConnectionStore((s) =>
    instanceId ? s.statuses[instanceId] : undefined,
  );
  const connectionEnabled = Boolean(instanceId);
  const connectionStatus = selectedConnectionStatus ?? DISCONNECTED_STATUS;
  const backendConnectionStatus = connectionStatus.status
    ?? (connectionStatus.connected ? 'connected' : 'disconnected');
  const connectCapability = connectionStatus.actions?.connect
    ?? DEFAULT_CONNECT_CAPABILITY;
  const disconnectCapability = connectionStatus.actions?.disconnect
    ?? DEFAULT_DISCONNECT_CAPABILITY;
  const orphanCleanupAvailable = backendConnectionStatus === 'disconnected'
    && disconnectCapability.enabled;

  const pageActive = usePageActive();

  // 进入列表页：60s staleness 兜底拉取（与后端可见集合缓存 TTL 对齐）。
  useEffect(() => {
    if (pageActive && authenticatedScope) {
      fetchEmployeesIfStale().catch(() => {
        /* error stored in store */
      });
    }
  }, [pageActive, authenticatedScope, fetchEmployeesIfStale]);

  // 在线/离线探测：离线 → 在线跳变时 store 会自动校准 refetch。
  useEffect(() => {
    setOnline(navigator.onLine);
    const handleOnline = () => setOnline(true);
    const handleOffline = () => setOnline(false);
    window.addEventListener('online', handleOnline);
    window.addEventListener('offline', handleOffline);
    return () => {
      window.removeEventListener('online', handleOnline);
      window.removeEventListener('offline', handleOffline);
    };
  }, [setOnline]);

  const handleConnect = async (employee: DigitalEmployeeBrief) => {
    if (!instanceId) return;
    clearError();
    try {
      const res = await selectEmployeeAndConnect(instanceId, employee.id);
      if (res) {
        message.success(t('managerAccount.employees.connectSuccess', { name: res.name }));
      }
    } catch (e) {
      const err = e as ManagerError;
      if (err?.kind === 'not_found_or_no_permission') {
        // store 已剔除该项并触发校准 refetch；这里只提示用户。
        message.warning(t('managerAccount.employees.visibilityRevoked', { name: employee.name }));
      }
      /* 其余错误 & paymentRequired 已存入 store，由顶部 Alert 呈现 */
    }
  };

  const handleDisconnectEmployee = async () => {
    try {
      if (!instanceId) return;
      await disconnectSmcp(instanceId);
      message.success(t('managerAccount.employees.disconnectSuccess'));
    } catch {
      message.error(t('computer.messages.connectionOperationFailed'));
    }
  };

  const isConnectedEmployee = (emp: DigitalEmployeeBrief): boolean => {
    if (backendConnectionStatus === 'disconnected') return false;
    if (connectionStatus.profile_name === `manager:${emp.id}`) return true;
    return !!(emp.robotId && connectionStatus.office_id === emp.robotId);
  };

  const handleLogout = async () => {
    try {
      await logout();
      message.success(t('managerAccount.login.logoutSuccess'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handlePaymentRedirect = async () => {
    if (paymentRequired?.redirectUrl) {
      try {
        await openExternal(paymentRequired.redirectUrl);
      } catch (e) {
        message.error(String(e));
      }
    }
  };

  return (
    <Card>
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        <div
          style={{
            display: 'flex',
            justifyContent: showIdentitySummary ? 'space-between' : 'flex-end',
            alignItems: 'flex-start',
          }}
        >
          {showIdentitySummary && (
            <div>
              <Title level={4} style={{ marginBottom: 4 }}>
                {t('managerAccount.employees.title')}
              </Title>
              {session && (
                <Text type="secondary">
                  {t('managerAccount.employees.signedInAs', { name: session.accountName })}
                </Text>
              )}
            </div>
          )}
          <Space>
            <Button icon={<ReloadOutlined />} onClick={() => fetchEmployees()} loading={loading}>
              {t('common.refresh')}
            </Button>
            {showIdentityActions && (
              <Button icon={<LogoutOutlined />} danger onClick={handleLogout} loading={loading}>
                {t('managerAccount.login.logout')}
              </Button>
            )}
          </Space>
        </div>

        {!online && (
          <Alert
            type="warning"
            showIcon
            message={t('managerAccount.employees.offlineBanner')}
          />
        )}

        {paymentRequired && (
          <Alert
            type="warning"
            showIcon
            message={t('manager.errors.payment_required')}
            description={
              <Space direction="vertical">
                <Text>{paymentRequired.message}</Text>
                {paymentRequired.redirectUrl && (
                  <Button type="primary" size="small" onClick={handlePaymentRedirect}>
                    {t('managerAccount.employees.renew')}
                  </Button>
                )}
              </Space>
            }
            closable
            onClose={dismissPaymentRequired}
          />
        )}

        {error && error.kind !== 'payment_required' && (
          <Alert
            type="error"
            showIcon
            message={t(errorI18nKey(error))}
            closable
            onClose={clearError}
          />
        )}

        {orphanCleanupAvailable && (
          <Alert
            type="warning"
            showIcon
            message={t('managerAccount.employees.connectionCleanupRequired')}
            description={t('managerAccount.employees.connectionCleanupGuidance')}
            action={(
              <Button
                danger
                icon={<DisconnectOutlined />}
                onClick={handleDisconnectEmployee}
              >
                {t('managerAccount.employees.disconnect')}
              </Button>
            )}
          />
        )}

        {!loading && employees.length === 0 && !error ? (
          <Empty description={t('managerAccount.employees.empty')} />
        ) : (
          <List
            bordered
            loading={loading}
            dataSource={employees}
            rowKey={(emp) => emp.id}
            renderItem={(emp) => {
              const connectable = isConnectable(emp);
              const isConnectionTarget = isConnectedEmployee(emp);
              const isConnecting = backendConnectionStatus === 'connecting'
                && connectionStatus.operation_target?.employee_id === emp.id;
              const isDisconnecting = backendConnectionStatus === 'disconnecting'
                && isConnectionTarget;
              const isConnected = backendConnectionStatus === 'connected' && isConnectionTarget;
              const connectButton = (
                <Button
                  key="connect"
                  type="primary"
                  icon={<LinkOutlined />}
                  disabled={!connectable || !connectCapability.enabled}
                  loading={isConnecting}
                  onClick={() => handleConnect(emp)}
                >
                  {t('managerAccount.employees.connect')}
                </Button>
              );
              return (
                <List.Item
                  actions={
                    connectionEnabled
                      ? [
                          isConnected || isDisconnecting ? (
                            <Button
                              key="disconnect"
                              danger
                              icon={<DisconnectOutlined />}
                              disabled={!disconnectCapability.enabled}
                              loading={isDisconnecting}
                              onClick={handleDisconnectEmployee}
                            >
                              {t('managerAccount.employees.disconnect')}
                            </Button>
                          ) : (
                            connectButton
                          ),
                        ]
                      : []
                  }
                >
                  <List.Item.Meta
                    avatar={<RobotOutlined style={{ fontSize: 24 }} />}
                    title={
                      <Space size={4} wrap>
                        <Text strong>{emp.name}</Text>
                        {connectionEnabled && isConnected && (
                          <Tag icon={<CheckCircleOutlined />} color="success">
                            {t('managerAccount.employees.connected')}
                          </Tag>
                        )}
                        {emp.status && (
                          <Tag color={employeeStatusTagColor(emp.status)}>
                            {t(employeeStatusI18nKey(emp.status), {
                              defaultValue: t('managerAccount.employees.status.unknown'),
                            })}
                          </Tag>
                        )}
                        {emp.templateDisplayName && <Tag>{emp.templateDisplayName}</Tag>}
                        {emp.templateType && <Tag color="purple">{emp.templateType}</Tag>}
                      </Space>
                    }
                    description={
                      <Descriptions size="small" column={1} colon={false}>
                        <Descriptions.Item label={t('managerAccount.employees.department')}>
                          {emp.departments && emp.departments.length > 0 ? (
                            <Space direction="vertical" size={0}>
                              {emp.departments.map((d) => (
                                <Text key={d.id}>{formatDeptBreadcrumb(d)}</Text>
                              ))}
                            </Space>
                          ) : (
                            <Text type="secondary">
                              {t('managerAccount.employees.noDepartment')}
                            </Text>
                          )}
                        </Descriptions.Item>
                        {emp.robotId && (
                          <Descriptions.Item
                            label={t('managerAccount.employees.robotId')}
                            contentStyle={{ fontFamily: 'monospace' }}
                          >
                            {emp.robotId}
                          </Descriptions.Item>
                        )}
                        {emp.namespace && (
                          <Descriptions.Item label={t('managerAccount.employees.namespace')}>
                            {emp.namespace}
                          </Descriptions.Item>
                        )}
                        {emp.clusterName && (
                          <Descriptions.Item label={t('managerAccount.employees.cluster')}>
                            {emp.clusterName}
                          </Descriptions.Item>
                        )}
                        {emp.description && (
                          <Descriptions.Item label={t('managerAccount.employees.description')}>
                            {emp.description}
                          </Descriptions.Item>
                        )}
                      </Descriptions>
                    }
                  />
                </List.Item>
              );
            }}
          />
        )}
      </Space>
    </Card>
  );
}
