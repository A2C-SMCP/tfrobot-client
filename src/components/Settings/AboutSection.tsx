import { useEffect } from 'react';
import { Descriptions, Space, Button } from 'antd';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-shell';
import { useSettingsStore } from '@/stores/settingsStore';

export function AboutSection() {
  const { t } = useTranslation();
  const { appInfo, fetchAppInfo } = useSettingsStore();

  useEffect(() => {
    fetchAppInfo();
  }, []);

  return (
    <div>
      <Descriptions bordered column={1}>
        <Descriptions.Item label={t('settings.appVersion')}>
          {appInfo?.version ?? '—'}
        </Descriptions.Item>
        <Descriptions.Item label={t('settings.sdkVersion')}>
          {appInfo?.smcp_computer_version ?? '—'}
        </Descriptions.Item>
        <Descriptions.Item label={t('settings.license')}>MIT</Descriptions.Item>
      </Descriptions>

      <Space style={{ marginTop: 16 }}>
        <Button type="link" onClick={() => open('https://github.com/nicepkg/tfrobot-client')}>
          {t('settings.feedback')}
        </Button>
      </Space>
    </div>
  );
}
