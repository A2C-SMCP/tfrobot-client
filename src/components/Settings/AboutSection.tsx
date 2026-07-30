import { useEffect } from 'react';
import { App, Descriptions, Space, Button } from 'antd';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-shell';
import { check } from '@tauri-apps/plugin-updater';
import { useSettingsStore } from '@/stores/settingsStore';

export function AboutSection() {
  const { t } = useTranslation();
  const { modal, message } = App.useApp();
  const { appInfo, fetchAppInfo } = useSettingsStore();

  useEffect(() => {
    fetchAppInfo();
  }, []);

  const checkForUpdates = async () => {
    try {
      const update = await check();
      if (update) {
        modal.confirm({
          title: t('settings.updateAvailable'),
          content: `${t('settings.newVersion')}: ${update.version}`,
          onOk: async () => {
            await update.downloadAndInstall();
          },
        });
      } else {
        message.info(t('settings.upToDate'));
      }
    } catch (e) {
      message.error(String(e));
    }
  };

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
        <Button onClick={checkForUpdates}>{t('settings.checkUpdate')}</Button>
        <Button type="link" onClick={() => open('https://github.com/nicepkg/tfrobot-client')}>
          {t('settings.feedback')}
        </Button>
      </Space>
    </div>
  );
}
