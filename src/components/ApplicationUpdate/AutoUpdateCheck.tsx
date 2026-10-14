import { App, Space, Typography } from 'antd';
import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { Update } from '@tauri-apps/plugin-updater';
import {
  claimAutomaticUpdateCheck,
  checkForApplicationUpdate,
  closeUpdate,
  createUpdateCorrelationId,
  recordUpdateActivity,
} from '@/services/applicationUpdater';
import { presentUpdatePrompt } from './updatePrompt';

type AutomaticCheckLease = {
  promise: Promise<Update | null>;
  take: () => Update | null;
  release: () => void;
};

type AutomaticCheckState = {
  promise: Promise<Update | null>;
  consumers: number;
  update: Update | null | undefined;
  claimed: boolean;
};

let automaticCheckState: AutomaticCheckState | null = null;

function startAutomaticCheck(): AutomaticCheckLease {
  if (automaticCheckState?.consumers === 0 && automaticCheckState.update !== undefined) {
    automaticCheckState = null;
  }
  if (!automaticCheckState) {
    const state: AutomaticCheckState = {
      promise: Promise.resolve(null) as Promise<Update | null>,
      consumers: 0,
      update: undefined,
      claimed: false,
    };
    state.promise = (async () => {
      const correlationId = createUpdateCorrelationId();
      try {
        if (!await claimAutomaticUpdateCheck()) return null;
        const update = await checkForApplicationUpdate();
        if (!update) return null;
        return update;
      } catch (error) {
        await recordUpdateActivity('check_failed', null, correlationId, 'automatic', String(error));
        return null;
      }
    })().then((update) => {
      state.update = update;
      if (state.consumers === 0 && update && !state.claimed) {
        state.claimed = true;
        void closeUpdate(update);
      }
      if (state.consumers === 0 && automaticCheckState === state) automaticCheckState = null;
      return update;
    });
    automaticCheckState = state;
  }

  const state = automaticCheckState;
  state.consumers += 1;
  let released = false;
  return {
    promise: state.promise,
    take: () => {
      if (released || state.claimed || !state.update) return null;
      state.claimed = true;
      return state.update;
    },
    release: () => {
      if (released) return;
      released = true;
      state.consumers -= 1;
      if (state.consumers === 0 && state.update && !state.claimed) {
        state.claimed = true;
        void closeUpdate(state.update);
      }
      if (state.consumers === 0 && state.update !== undefined && automaticCheckState === state) {
        automaticCheckState = null;
      }
    },
  };
}

function updateContent(version: string, body: string | undefined, t: (key: string) => string) {
  return (
    <Space direction="vertical" size="small">
      <Typography.Text>{`${t('settings.newVersion')}: ${version}`}</Typography.Text>
      {body && (
        <Typography.Paragraph style={{ whiteSpace: 'pre-wrap', marginBottom: 0 }}>
          {body}
        </Typography.Paragraph>
      )}
    </Space>
  );
}

/** Performs one best-effort startup check. The persisted claim is the 24-hour gate. */
export function AutoUpdateCheck() {
  const { modal } = App.useApp();
  const { t } = useTranslation();
  const confirmation = useRef<{ destroy: () => void } | null>(null);

  useEffect(() => {
    let disposed = false;
    const lease = startAutomaticCheck();
    const run = async () => {
      await lease.promise;
      if (disposed) {
        lease.release();
        return;
      }
      const update = lease.take();
      lease.release();
      if (!update) return;
      confirmation.current = presentUpdatePrompt({
        update,
        modal,
        title: t('settings.updateAvailable'),
        content: updateContent(update.version, update.body, t),
        cancelText: t('settings.updateLater'),
        trigger: 'automatic',
        isActive: () => !disposed,
        onInstallError: () => undefined,
      });
    };

    void run();

    return () => {
      disposed = true;
      lease.release();
      confirmation.current?.destroy();
      confirmation.current = null;
    };
  }, [modal, t]);

  return null;
}
