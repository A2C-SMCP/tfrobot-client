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
  Tooltip,
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
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import {
  useManagerStore,
  type DepartmentRef,
  type DigitalEmployeeBrief,
  type ManagerError,
} from '@/stores/managerStore';
import { useConnectionStore } from '@/stores/connectionStore';

const { Title, Text } = Typography;

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

export function EmployeeList() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    session,
    employees,
    selectedEmployeeId,
    loading,
    error,
    paymentRequired,
    online,
    fetchEmployees,
    fetchEmployeesIfStale,
    setOnline,
    selectEmployeeAndConnect,
    logout,
    clearError,
    dismissPaymentRequired,
  } = useManagerStore();
  const connectionStatus = useConnectionStore((s) => s.status);
  const fetchConnectionStatus = useConnectionStore((s) => s.fetchStatus);
  const disconnectSmcp = useConnectionStore((s) => s.disconnect);

  useEffect(() => {
    fetchConnectionStatus().catch(() => {
      /* noop */
    });
  }, [fetchConnectionStatus]);

  // 后端在连接 / 断开 / 后台预刷新重连时 emit 'connection'，据此刷新连接状态徽标（TFRC-11）。
  useEffect(() => {
    const unlisten = listen('connection', () => {
      fetchConnectionStatus().catch(() => {
        /* noop */
      });
    });
    return () => {
      unlisten.then((off) => off()).catch(() => {});
    };
  }, [fetchConnectionStatus]);

  // 进入列表页：60s staleness 兜底拉取（与后端可见集合缓存 TTL 对齐）。
  useEffect(() => {
    if (session) {
      fetchEmployeesIfStale().catch(() => {
        /* error stored in store */
      });
    }
  }, [session, fetchEmployeesIfStale]);

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
    clearError();
    try {
      const res = await selectEmployeeAndConnect(employee.id);
      if (res) {
        message.success(t('managerAccount.employees.connectSuccess', { name: res.name }));
        // 刷新连接状态，让 UI 上"已连接"标识立即生效
        await fetchConnectionStatus();
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
      await disconnectSmcp();
      message.success(t('managerAccount.employees.disconnectSuccess'));
    } catch (e) {
      message.error(String(e));
    }
  };

  /** 判定某个 employee 是否正是当前 SMCP 连接的目标（按 office_id = robotId 匹配）。 */
  const isConnectedEmployee = (emp: DigitalEmployeeBrief): boolean =>
    !!(
      connectionStatus?.connected &&
      emp.robotId &&
      connectionStatus.office_id === emp.robotId
    );

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
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
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
          <Space>
            <Button icon={<ReloadOutlined />} onClick={() => fetchEmployees()} loading={loading}>
              {t('common.refresh')}
            </Button>
            <Button icon={<LogoutOutlined />} danger onClick={handleLogout} loading={loading}>
              {t('managerAccount.login.logout')}
            </Button>
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
            description={
              error.kind === 'network_error'
                ? error.detail
                : error.kind === 'other'
                ? `HTTP ${error.detail.status}: ${error.detail.body}`
                : undefined
            }
            closable
            onClose={clearError}
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
              const isSelecting = selectedEmployeeId === emp.id && loading;
              const isConnected = isConnectedEmployee(emp);
              // 无 robotAccountId（历史/未回填实例）无法做 token-exchange → 禁用连接（TFRC-11 / TFRM-183）。
              const noRobotAccount = emp.robotAccountId == null;
              const connectButton = (
                <Button
                  key="connect"
                  type="primary"
                  icon={<LinkOutlined />}
                  disabled={!connectable || noRobotAccount}
                  loading={isSelecting}
                  onClick={() => handleConnect(emp)}
                >
                  {t('managerAccount.employees.connect')}
                </Button>
              );
              return (
                <List.Item
                  actions={[
                    isConnected ? (
                      <Button
                        key="disconnect"
                        danger
                        icon={<DisconnectOutlined />}
                        loading={loading}
                        onClick={handleDisconnectEmployee}
                      >
                        {t('managerAccount.employees.disconnect')}
                      </Button>
                    ) : noRobotAccount ? (
                      <Tooltip
                        key="connect"
                        title={t('managerAccount.employees.noRobotAccount')}
                      >
                        <span>{connectButton}</span>
                      </Tooltip>
                    ) : (
                      connectButton
                    ),
                  ]}
                >
                  <List.Item.Meta
                    avatar={<RobotOutlined style={{ fontSize: 24 }} />}
                    title={
                      <Space size={4} wrap>
                        <Text strong>{emp.name}</Text>
                        {isConnected && (
                          <Tag icon={<CheckCircleOutlined />} color="success">
                            {t('managerAccount.employees.connected')}
                          </Tag>
                        )}
                        {emp.status && (
                          <Tag color={employeeStatusTagColor(emp.status)}>{emp.status}</Tag>
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
