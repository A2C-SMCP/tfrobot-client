import { App, Button, Card, Descriptions, Space, Switch, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';

const { Text } = Typography;

interface ComputerRuntimeSettingsProps {
  instance: ComputerInstance;
}

export function ComputerRuntimeSettings({ instance }: ComputerRuntimeSettingsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { loading, updateConnectionPolicy } = useComputerStore();
  const target = instance.connectionPolicy.target;
  const autoConnect = instance.connectionPolicy.auto_connect;

  const handleAutoConnectChange = async (checked: boolean) => {
    try {
      await updateConnectionPolicy(instance.id, {
        target: target ?? null,
        auto_connect: checked,
      });
      message.success(t('common.saved'));
    } catch (e) {
      message.error(String(e));
    }
  };

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small" title={t('computer.runtime.policyTitle')}>
        <Descriptions column={1} size="small">
          <Descriptions.Item label={t('computer.runtime.instance')}>
            <Space direction="vertical" size={0}>
              <Text strong>{instance.name}</Text>
              <Text type="secondary" copyable>{instance.id}</Text>
            </Space>
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.connectionTarget')}>
            {target ? (
              <Text code>
                {target.type}:{target.id}
              </Text>
            ) : (
              <Text type="secondary">{t('computer.runtime.noConnectionTarget')}</Text>
            )}
          </Descriptions.Item>
          <Descriptions.Item label={t('connection.form.autoConnect')}>
            <Space>
              <Switch
                checked={autoConnect}
                disabled={!target}
                loading={loading}
                onChange={handleAutoConnectChange}
              />
              <Text type="secondary">
                {target
                  ? t('computer.runtime.autoConnectDescription')
                  : t('computer.runtime.autoConnectRequiresTarget')}
              </Text>
            </Space>
          </Descriptions.Item>
        </Descriptions>
      </Card>

      <Card size="small" title={t('computer.runtime.lifecycleTitle')}>
        <Space direction="vertical" size={8}>
          <Text>{t('computer.runtime.startBehavior')}</Text>
          <Text type="secondary">{t('computer.runtime.globalRuntimeHint')}</Text>
          <Button onClick={() => useComputerStore.getState().startInstance(instance.id)} loading={loading}>
            {t('computer.start')}
          </Button>
        </Space>
      </Card>
    </Space>
  );
}
