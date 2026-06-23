import { useEffect, useMemo, useState } from 'react';
import { App, Button, Card, Descriptions, Select, Space, Switch, Tabs, Tag, Typography } from 'antd';
import { ApiOutlined, DisconnectOutlined, LinkOutlined, ReloadOutlined, UserOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { ManagerAccount } from '@/components/ManagerAccount';
import { useComputerStore } from '@/stores/computerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';

const { Text } = Typography;

interface RobotConnectionPanelProps {
  instanceId: string;
}

export function RobotConnectionPanel({ instanceId }: RobotConnectionPanelProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { instances, updateManualConnectionPolicy } = useComputerStore();
  const { getStatus, fetchStatus, disconnect, loading: connectionLoading } = useConnectionStore();
  const {
    manualTargets,
    fetchManualTargets,
    connectTarget,
    loading: targetLoading,
  } = useConnectionTargetStore();
  const [selectedTargetId, setSelectedTargetId] = useState<string>();
  const [autoConnect, setAutoConnect] = useState(false);
  const selectedInstance = instances.find((instance) => instance.id === instanceId);

  useEffect(() => {
    fetchStatus(instanceId);
    fetchManualTargets();
  }, [fetchStatus, fetchManualTargets, instanceId]);

  useEffect(() => {
    setSelectedTargetId(selectedInstance?.manualConnectionPolicy.target_id ?? undefined);
    setAutoConnect(selectedInstance?.manualConnectionPolicy.auto_connect ?? false);
  }, [
    selectedInstance?.manualConnectionPolicy.auto_connect,
    selectedInstance?.manualConnectionPolicy.target_id,
  ]);

  const selectedTarget = useMemo(
    () => manualTargets.find((target) => target.id === selectedTargetId),
    [manualTargets, selectedTargetId],
  );
  const status = getStatus(instanceId);

  const handleDisconnect = async () => {
    await disconnect(instanceId);
    message.success(t('connection.messages.disconnected'));
  };

  const handleConnect = async () => {
    if (!selectedTargetId) return;
    await updateManualConnectionPolicy(instanceId, {
      target_id: selectedTargetId,
      auto_connect: autoConnect,
    });
    await connectTarget(instanceId, selectedTargetId);
    message.success(t('connection.messages.connected'));
  };

  const handleSavePolicy = async () => {
    await updateManualConnectionPolicy(instanceId, {
      target_id: selectedTargetId ?? null,
      auto_connect: autoConnect,
    });
    message.success(t('common.saved'));
  };

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
              <Card size="small" title="Change Connection Target">
                <Space wrap>
                  <Select
                    style={{ minWidth: 320 }}
                    value={selectedTargetId}
                    placeholder="Select a Manual SMCP target"
                    onChange={setSelectedTargetId}
                    options={manualTargets.map((target) => ({
                      value: target.id,
                      label: `${target.name} (${target.office_id})`,
                    }))}
                  />
                  <Button onClick={() => fetchManualTargets()}>{t('common.refresh')}</Button>
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
                    disabled={!selectedTarget || status.connected}
                    loading={targetLoading}
                    onClick={() => handleConnect().catch((e) => message.error(String(e)))}
                  >
                    {t('connection.connect')}
                  </Button>
                  {status.connected && (
                    <Text type="secondary">
                      Disconnect the current target before connecting another.
                    </Text>
                  )}
                </Space>
              </Card>
            ),
          },
        ]}
      />
    </Space>
  );
}
