import { useEffect, useMemo, useState } from 'react';
import { App, Button, Card, Descriptions, Select, Space, Tag, Typography } from 'antd';
import { DisconnectOutlined, LinkOutlined, ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useConnectionStore } from '@/stores/connectionStore';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';

const { Text } = Typography;

interface RobotConnectionPanelProps {
  instanceId: string;
}

export function RobotConnectionPanel({ instanceId }: RobotConnectionPanelProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { getStatus, fetchStatus, disconnect, loading: connectionLoading } = useConnectionStore();
  const {
    manualTargets,
    fetchManualTargets,
    connectTarget,
    loading: targetLoading,
  } = useConnectionTargetStore();
  const [selectedTargetId, setSelectedTargetId] = useState<string>();

  useEffect(() => {
    fetchStatus(instanceId);
    fetchManualTargets();
  }, [fetchStatus, fetchManualTargets, instanceId]);

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
    await connectTarget(instanceId, selectedTargetId);
    message.success(t('connection.messages.connected'));
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
            <Text type="secondary">Disconnect the current target before connecting another.</Text>
          )}
        </Space>
      </Card>
    </Space>
  );
}
