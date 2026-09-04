import { useEffect } from 'react';
import { App, Descriptions, Space, Button } from 'antd';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-shell';
import { check } from '@tauri-apps/plugin-updater';
import { invoke } from '@tauri-apps/api/core';
import { useSettingsStore } from '@/stores/settingsStore';
import { warn as logWarn } from '@/utils/logger';

type UpdateActivity = 'check_failed' | 'install_started' | 'install_succeeded' | 'install_failed';

async function recordUpdateActivity(
  activity: UpdateActivity,
  targetVersion: string | null,
  correlationId: string,
  error: string | null = null,
) {
  try {
    await invoke('record_application_update_activity', {
      activity,
      targetVersion,
      correlationId,
      error,
    });
  } catch (auditError) {
    void logWarn(`Failed to persist application update activity: ${String(auditError)}`);
  }
}

export function AboutSection() {
  const { t } = useTranslation();
  const { modal, message } = App.useApp();
  const { appInfo, fetchAppInfo } = useSettingsStore();

  useEffect(() => {
    fetchAppInfo();
  }, [fetchAppInfo]);

  const checkForUpdates = async () => {
    const correlationId = crypto.randomUUID();
    try {
      const update = await check();
      if (update) {
        modal.confirm({
          title: t('settings.updateAvailable'),
          content: `${t('settings.newVersion')}: ${update.version}`,
          onOk: async () => {
            await recordUpdateActivity('install_started', update.version, correlationId);
            try {
              await update.downloadAndInstall();
            } catch (error) {
              await recordUpdateActivity('install_failed', update.version, correlationId, String(error));
              message.error(String(error));
              return;
            }
            await recordUpdateActivity('install_succeeded', update.version, correlationId);
          },
        });
      } else {
        message.info(t('settings.upToDate'));
      }
    } catch (e) {
      await recordUpdateActivity('check_failed', null, correlationId, String(e));
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
