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
  ExclamationCircleOutlined,
} from '@ant-design/icons';
import { open as openExternal } from '@tauri-apps/plugin-shell';
import { useTranslation } from 'react-i18next';
import {
  useManagerStore,
  type DigitalEmployeeBrief,
  type ManagerError,
} from '@/stores/managerStore';
import { useConnectionStore } from '@/stores/connectionStore';

const { Title, Text } = Typography;

function errorI18nKey(err: ManagerError): string {
  return `manager.errors.${err.kind}`;
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
  const { modal, message } = App.useApp();
  const {
    session,
    employees,
    selectedEmployeeId,
    loading,
    error,
    paymentRequired,
    fetchEmployees,
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

  useEffect(() => {
    if (session) {
      fetchEmployees().catch(() => {
        /* error stored in store */
      });
    }
  }, [session, fetchEmployees]);

  const resolveNameConflict = (existingName: string): Promise<'overwrite' | 'copy' | 'cancel'> =>
    new Promise((resolve) => {
      modal.confirm({
        title: t('managerAccount.employees.conflictTitle'),
        icon: <ExclamationCircleOutlined />,
        content: t('managerAccount.employees.conflictDescription', { name: existingName }),
        okText: t('managerAccount.employees.overwrite'),
        cancelText: t('managerAccount.employees.createCopy'),
        closable: true,
        onOk: () => resolve('overwrite'),
        onCancel: (close) => {
          if (typeof close === 'function') {
            resolve('copy');
          } else {
            resolve('cancel');
          }
        },
      });
    });

  const handleConnect = async (employee: DigitalEmployeeBrief) => {
    clearError();
    try {
      const res = await selectEmployeeAndConnect(employee.id, resolveNameConflict);
      if (res) {
        message.success(t('managerAccount.employees.connectSuccess', { name: res.profileName }));
        // 刷新连接状态，让 UI 上"已连接"标识立即生效
        await fetchConnectionStatus();
      }
    } catch {
      /* error & paymentRequired already stored */
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
                    ) : (
                      <Button
                        key="connect"
                        type="primary"
                        icon={<LinkOutlined />}
                        disabled={!connectable}
                        loading={isSelecting}
                        onClick={() => handleConnect(emp)}
                      >
                        {t('managerAccount.employees.connect')}
                      </Button>
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
