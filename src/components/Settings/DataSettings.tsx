import { Form, InputNumber, Button, Space, Popconfirm } from 'antd';
import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';
import { useLogStore } from '@/stores/logStore';

export function DataSettings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSettingsStore();
  const { clearLogs } = useLogStore();

  const handleRetentionChange = (days: number | null) => {
    if (!settings || !days) return;
    updateSettings({ ...settings, log_retention_days: days });
  };

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Form layout="vertical">
        <Form.Item label={t('settings.logRetention')}>
          <InputNumber
            min={1}
            max={365}
            value={settings?.log_retention_days ?? 30}
            addonAfter={t('settings.days')}
            onChange={handleRetentionChange}
          />
        </Form.Item>
      </Form>

      <Popconfirm title={t('logs.confirmClear')} onConfirm={() => clearLogs()}>
        <Button danger>{t('logs.clear')}</Button>
      </Popconfirm>

      <Popconfirm title={t('settings.resetConfirm')} onConfirm={() => clearLogs()}>
        <Button danger type="primary">{t('settings.reset')}</Button>
      </Popconfirm>
    </Space>
  );
}
