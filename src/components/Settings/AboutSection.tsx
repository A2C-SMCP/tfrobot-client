import { usePageActive, usePageAction } from '@/components/Navigation/pageActivityState';
import { useEffect, useLayoutEffect, useRef } from 'react';
import { App, Descriptions, Space, Button, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-shell';
import { useSettingsStore } from '@/stores/settingsStore';
import {
  checkForApplicationUpdate,
  closeUpdate,
  createUpdateCorrelationId,
  recordUpdateActivity,
} from '@/services/applicationUpdater';
import { presentUpdatePrompt } from '@/components/ApplicationUpdate/updatePrompt';

export function AboutSection() {
  const { t } = useTranslation();
  const { modal, message } = App.useApp();
  const active = usePageActive();
  const action = usePageAction();
  const pendingConfirmation = useRef<(() => void) | null>(null);
  const checkSequence = useRef(0);
  useLayoutEffect(() => {
    if (!active) { pendingConfirmation.current?.(); pendingConfirmation.current = null; }
    return () => { pendingConfirmation.current?.(); pendingConfirmation.current = null; };
  }, [active]);
  const { appInfo, fetchAppInfo } = useSettingsStore();

  useEffect(() => {
    fetchAppInfo();
  }, [fetchAppInfo]);

  const checkForUpdates = async () => {
    const pageCurrent = action();
    const sequence = ++checkSequence.current;
    const current = () => pageCurrent() && sequence === checkSequence.current;
    pendingConfirmation.current?.();
    pendingConfirmation.current = null;
    const correlationId = createUpdateCorrelationId();
    try {
      const update = await checkForApplicationUpdate();
      if (update) {
        if (!current()) { await closeUpdate(update); return; }
        const confirmation = presentUpdatePrompt({
          update,
          modal,
          title: t('settings.updateAvailable'),
          content: (
            <Space direction="vertical" size="small">
              <Typography.Text>{`${t('settings.newVersion')}: ${update.version}`}</Typography.Text>
              <Typography.Text>{t('permissions.update')}</Typography.Text>
              {update.body && (
                <Typography.Paragraph style={{ whiteSpace: 'pre-wrap', marginBottom: 0 }}>
                  {update.body}
                </Typography.Paragraph>
              )}
            </Space>
          ),
          cancelText: t('settings.updateLater'),
          trigger: 'manual',
          isActive: current,
          onInstallError: () => {
            if (current()) message.error(t('settings.updateFailed'));
          },
        });
        pendingConfirmation.current = confirmation.destroy;
      } else if (current()) {
        message.info(t('settings.upToDate'));
      }
    } catch (e) {
      await recordUpdateActivity('check_failed', null, correlationId, 'manual', String(e));
      if (current()) message.error(t('settings.updateCheckFailed'));
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
