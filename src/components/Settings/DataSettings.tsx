import { PagePopconfirm as Popconfirm } from '@/components/Navigation/PageOverlays';
import { Form, InputNumber, Button, Space, Select } from 'antd';
import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';
import { useActivityStore } from '@/stores/activityStore';

export function DataSettings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSettingsStore();
  const { clearActivity } = useActivityStore();

  const updateNumber = (
    key: 'diagnostic_retention_days' | 'activity_retention_days' | 'tool_history_retention_days',
    days: number | null,
  ) => {
    if (settings && days) void updateSettings({ ...settings, [key]: days });
  };

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Form layout="vertical">
        <Form.Item label="Diagnostic level">
          <Select
            value={settings?.diagnostic_log_level ?? 'info'}
            options={['error', 'warn', 'info', 'debug', 'trace'].map((value) => ({
              label: value.toUpperCase(), value,
            }))}
            onChange={(diagnostic_log_level) => settings && void updateSettings({ ...settings, diagnostic_log_level })}
          />
        </Form.Item>
        <Form.Item label="Diagnostic file retention">
          <InputNumber
            min={1}
            max={365}
            value={settings?.diagnostic_retention_days ?? 7}
            addonAfter={t('settings.days')}
            onChange={(days) => updateNumber('diagnostic_retention_days', days)}
          />
        </Form.Item>
        <Form.Item label="Activity retention">
          <InputNumber
            min={1}
            max={365}
            value={settings?.activity_retention_days ?? 30}
            addonAfter={t('settings.days')}
            onChange={(days) => updateNumber('activity_retention_days', days)}
          />
        </Form.Item>
        <Form.Item label="Tool history retention">
          <InputNumber
            min={1}
            max={365}
            value={settings?.tool_history_retention_days ?? 90}
            addonAfter={t('settings.days')}
            onChange={(days) => updateNumber('tool_history_retention_days', days)}
          />
        </Form.Item>
      </Form>

      <Popconfirm title={t('logs.confirmClear')} onConfirm={() => clearActivity({ kind: 'all' })}>
        <Button danger>{t('logs.clear')}</Button>
      </Popconfirm>

      <Popconfirm title={t('settings.resetConfirm')} onConfirm={() => clearActivity({ kind: 'all' })}>
        <Button danger type="primary">{t('settings.reset')}</Button>
      </Popconfirm>
    </Space>
  );
}
