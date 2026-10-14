import type { ModalFuncProps } from 'antd';
import type { ReactNode } from 'react';
import type { Update } from '@tauri-apps/plugin-updater';
import {
  closeUpdate,
  createUpdateCorrelationId,
  deferUpdate,
  recordUpdateActivity,
  type UpdateTrigger,
} from '@/services/applicationUpdater';

type UpdateModal = {
  confirm: (props: ModalFuncProps) => { destroy: () => void };
};

let activePrompt: { update: Update; controller: { destroy: () => void } } | null = null;

export function presentUpdatePrompt(options: {
  update: Update;
  modal: UpdateModal;
  title: ReactNode;
  content: ReactNode;
  cancelText: string;
  trigger: UpdateTrigger;
  isActive: () => boolean;
  onInstallError: (error: unknown) => void;
}): { destroy: () => void } {
  if (activePrompt?.update === options.update) return activePrompt.controller;

  const correlationId = createUpdateCorrelationId();
  let started = false;
  let released = false;
  const clearPrompt = () => {
    if (activePrompt?.update === options.update && activePrompt.controller === controller) {
      activePrompt = null;
    }
  };
  const release = () => {
    if (!released) {
      released = true;
      void closeUpdate(options.update);
    }
  };
  const confirmation = options.modal.confirm({
    title: options.title,
    content: options.content,
    cancelText: options.cancelText,
    onCancel: () => {
      clearPrompt();
      void deferUpdate(options.update.version);
      release();
    },
    onOk: async () => {
      if (!options.isActive() || started) return;
      started = true;
      await recordUpdateActivity(
        'install_started',
        options.update.version,
        correlationId,
        options.trigger,
      );
      try {
        await options.update.downloadAndInstall();
        await recordUpdateActivity(
          'install_succeeded',
          options.update.version,
          correlationId,
          options.trigger,
        );
      } catch (error) {
        await recordUpdateActivity(
          'install_failed',
          options.update.version,
          correlationId,
          options.trigger,
          String(error),
        );
        options.onInstallError(error);
      } finally {
        clearPrompt();
        release();
      }
    },
  });
  const controller = {
    destroy: () => {
      confirmation.destroy();
      clearPrompt();
      if (!started) release();
    },
  };
  activePrompt = { update: options.update, controller };
  return controller;
}
