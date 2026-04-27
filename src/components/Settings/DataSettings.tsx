import { App, Form, InputNumber, Button, Space, Popconfirm } from 'antd';
import { ExportOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { useSettingsStore } from '@/stores/settingsStore';
import { useLogStore } from '@/stores/logStore';
import { useMcpStore } from '@/stores/mcpStore';

export function DataSettings() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { settings, updateSettings } = useSettingsStore();
  const { clearLogs } = useLogStore();
  const { exportConfig } = useMcpStore();

  const handleRetentionChange = (days: number | null) => {
    if (!settings || !days) return;
    updateSettings({ ...settings, log_retention_days: days });
  };

  const handleExportAll = async () => {
    const path = await save({
      defaultPath: 'tfrobot-config.json',
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    if (path) {
      try {
        await exportConfig(path);
        message.success(t('mcp.messages.exportSuccess'));
      } catch {
        message.error(t('common.error'));
      }
    }
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

      <Button icon={<ExportOutlined />} onClick={handleExportAll}>
        {t('settings.exportAll')}
      </Button>

      <Popconfirm title={t('logs.confirmClear')} onConfirm={() => clearLogs()}>
        <Button danger>{t('logs.clear')}</Button>
      </Popconfirm>

      <Popconfirm title={t('settings.resetConfirm')} onConfirm={() => clearLogs()}>
        <Button danger type="primary">{t('settings.reset')}</Button>
      </Popconfirm>
    </Space>
  );
}
