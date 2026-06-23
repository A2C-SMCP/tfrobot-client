import { useEffect, useMemo, useState } from 'react';
import { App, Button, Card, Descriptions, Select, Space, Switch, Tabs, Tag, Typography } from 'antd';
import { ApiOutlined, DisconnectOutlined, LinkOutlined, ReloadOutlined, UserOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { ManagerAccount } from '@/components/ManagerAccount';
import {
  useComputerStore,
  type ComputerConnectionTarget,
  type ComputerConnectionTargetType,
} from '@/stores/computerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';
import { useManagerStore } from '@/stores/managerStore';

const { Text } = Typography;

interface RobotConnectionPanelProps {
  instanceId: string;
}

export function RobotConnectionPanel({ instanceId }: RobotConnectionPanelProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { instances, updateConnectionPolicy, connectSelectedTarget } = useComputerStore();
  const { getStatus, fetchStatus, disconnect, loading: connectionLoading } = useConnectionStore();
  const {
    manualTargets,
    fetchManualTargets,
    loading: targetLoading,
  } = useConnectionTargetStore();
  const { session, employees, fetchEmployeesIfStale } = useManagerStore();
  const [selectedTargetValue, setSelectedTargetValue] = useState<string>();
  const [autoConnect, setAutoConnect] = useState(false);
  const selectedInstance = instances.find((instance) => instance.id === instanceId);

  useEffect(() => {
    fetchStatus(instanceId);
    fetchManualTargets();
    if (session) {
      fetchEmployeesIfStale().catch(() => {
        /* manager error is stored in manager store */
      });
    }
  }, [fetchStatus, fetchManualTargets, fetchEmployeesIfStale, instanceId, session]);

  useEffect(() => {
    setSelectedTargetValue(targetToValue(selectedInstance?.connectionPolicy.target));
    setAutoConnect(selectedInstance?.connectionPolicy.auto_connect ?? false);
  }, [
    selectedInstance?.connectionPolicy.auto_connect,
    selectedInstance?.connectionPolicy.target,
  ]);

  const selectedTarget = useMemo(
    () => valueToTarget(selectedTargetValue),
    [selectedTargetValue],
  );
  const status = getStatus(instanceId);
  const canConnect = selectedInstance?.status === 'running' && Boolean(selectedTarget) && !status.connected;

  const handleDisconnect = async () => {
    await disconnect(instanceId);
    message.success(t('connection.messages.disconnected'));
  };

  const handleConnect = async () => {
    if (!selectedTarget) return;
    await updateConnectionPolicy(instanceId, {
      target: selectedTarget,
      auto_connect: autoConnect,
    });
    await connectSelectedTarget(instanceId);
    await fetchStatus(instanceId);
    message.success(t('connection.messages.connected'));
  };

  const handleSavePolicy = async () => {
    await updateConnectionPolicy(instanceId, {
      target: selectedTarget,
      auto_connect: autoConnect,
    });
    message.success(t('common.saved'));
  };

  const targetOptions = useMemo(() => {
    const managerOptions = employees.map((employee) => ({
      value: targetToValue({ type: 'manager_robot', id: String(employee.id) })!,
      label: employee.name,
    }));
    const manualOptions = manualTargets.map((target) => ({
      value: targetToValue({ type: 'manual_smcp', id: target.id })!,
      label: `${target.name} (${target.office_id})`,
    }));
    return [
      {
        label: 'Manager Robots',
        options: managerOptions,
      },
      {
        label: 'Manual SMCP',
        options: manualOptions,
      },
    ];
  }, [employees, manualTargets]);

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card
        size="small"
        title={
          <Space>
            <Tag color={status.connected ? 'success' : 'default'}>
              {status.connected ? t('connection.connected') : t('connection.disconnected')}
            </Tag>
            {t('connection.statusTitle')}
          </Space>
        }
        extra={
          status.connected ? (
            <Button
              danger
              icon={<DisconnectOutlined />}
              loading={connectionLoading}
              onClick={() => handleDisconnect().catch((e) => message.error(String(e)))}
            >
              {t('connection.disconnect')}
            </Button>
          ) : (
            <Button icon={<ReloadOutlined />} onClick={() => fetchStatus(instanceId)}>
              {t('common.refresh')}
            </Button>
          )
        }
      >
        {status.connected ? (
          <Descriptions size="small" column={2}>
            <Descriptions.Item label="Source">{status.source_type ?? '-'}</Descriptions.Item>
            <Descriptions.Item label="Target">
              {status.target_name ?? status.profile_name ?? '-'}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.table.url')}>
              {status.url}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.table.office')}>
              {status.office_id}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.table.computer')}>
              {status.computer_name}
            </Descriptions.Item>
            <Descriptions.Item label={t('connection.connectedAt')}>
              {status.connected_at ? new Date(status.connected_at).toLocaleString() : '-'}
            </Descriptions.Item>
          </Descriptions>
        ) : (
          <Text type="secondary">{t('connection.notConnected')}</Text>
        )}
      </Card>

      <Card size="small" title={t('computer.connectionActions.targetTitle')}>
        <Space wrap>
          <Select
            allowClear
            style={{ minWidth: 360 }}
            value={selectedTargetValue}
            placeholder={t('computer.connectionActions.selectTarget')}
            onChange={setSelectedTargetValue}
            options={targetOptions}
          />
          <Button onClick={() => {
            fetchManualTargets();
            if (session) fetchEmployeesIfStale(0);
          }}>
            {t('common.refresh')}
          </Button>
          <Space>
            <Switch checked={autoConnect} onChange={setAutoConnect} />
            <Text>{t('connection.form.autoConnect')}</Text>
          </Space>
          <Button
            disabled={targetLoading}
            onClick={() => handleSavePolicy().catch((e) => message.error(String(e)))}
          >
            {t('common.save')}
          </Button>
          <Button
            type="primary"
            icon={<LinkOutlined />}
            disabled={!canConnect}
            loading={targetLoading}
            onClick={() => handleConnect().catch((e) => message.error(String(e)))}
          >
            {t('connection.connect')}
          </Button>
          {!status.connected && selectedInstance?.status !== 'running' && (
            <Text type="secondary">{t('computer.connectionActions.requiresRunning')}</Text>
          )}
          {!status.connected && selectedInstance?.status === 'running' && !selectedTarget && (
            <Text type="secondary">{t('computer.connectionActions.requiresTarget')}</Text>
          )}
        </Space>
      </Card>

      <Tabs
        items={[
          {
            key: 'manager',
            label: (
              <>
                <UserOutlined /> Manager Robots
              </>
            ),
            children: <ManagerAccount instanceId={instanceId} />,
          },
          {
            key: 'manual',
            label: (
              <>
                <ApiOutlined /> Manual SMCP
              </>
            ),
            children: (
              <Card size="small">
                <Space>
                  <Text type="secondary">{t('computer.connectionActions.manualManagedGlobally')}</Text>
                  <Button onClick={() => fetchManualTargets()}>{t('common.refresh')}</Button>
                </Space>
              </Card>
            ),
          },
        ]}
      />
    </Space>
  );
}

function targetToValue(target?: ComputerConnectionTarget | null): string | undefined {
  if (!target) return undefined;
  return `${target.type}:${target.id}`;
}

function valueToTarget(value?: string): ComputerConnectionTarget | null {
  if (!value) return null;
  const [type, ...idParts] = value.split(':');
  const id = idParts.join(':');
  if (!id || (type !== 'manager_robot' && type !== 'manual_smcp')) return null;
  return { type: type as ComputerConnectionTargetType, id };
}
