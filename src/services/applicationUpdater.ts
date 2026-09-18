import { invoke } from '@tauri-apps/api/core';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { warn as logWarn } from '@/utils/logger';

export type UpdateTrigger = 'automatic' | 'manual';
export type UpdateActivity =
  | 'check_failed'
  | 'install_started'
  | 'install_succeeded'
  | 'install_failed';

export interface UpdatePreferences {
  lastAutomaticCheckAt?: number | null;
  deferredVersion?: string | null;
}

export function createUpdateCorrelationId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `update-${Date.now()}`;
}

export async function claimAutomaticUpdateCheck(): Promise<boolean> {
  return invoke<boolean>('claim_automatic_update_check');
}

let inFlightUpdateCheck: Promise<Update | null> | null = null;
let activeUpdate: Update | null = null;
let closingUpdate: Promise<void> | null = null;

/** Shares an in-flight updater check between startup and the About page. */
export function checkForApplicationUpdate(): Promise<Update | null> {
  if (closingUpdate) return closingUpdate.then(() => checkForApplicationUpdate());
  if (activeUpdate) return Promise.resolve(activeUpdate);
  if (!inFlightUpdateCheck) {
    inFlightUpdateCheck = check()
      .then((update) => {
        if (!update) return null;
        return getUpdatePreferences()
          .then(async (preferences) => {
            if (preferences.deferredVersion === update.version) {
              await closeUpdate(update);
              return null;
            }
            activeUpdate = update;
            return update;
          })
          .catch(async (error) => {
            await closeUpdate(update);
            throw error;
          });
      })
      .finally(() => { inFlightUpdateCheck = null; });
  }
  return inFlightUpdateCheck;
}

export async function getUpdatePreferences(): Promise<UpdatePreferences> {
  return invoke<UpdatePreferences>('get_update_preferences');
}

export async function deferUpdate(version: string): Promise<void> {
  try {
    await invoke('set_deferred_update_version', { version });
  } catch (error) {
    void logWarn(`Failed to persist deferred application update: ${String(error)}`);
  }
}

export async function recordUpdateActivity(
  activity: UpdateActivity,
  targetVersion: string | null,
  correlationId: string,
  trigger: UpdateTrigger,
  error: string | null = null,
): Promise<void> {
  try {
    await invoke('record_application_update_activity', {
      activity,
      targetVersion,
      correlationId,
      trigger,
      error,
    });
  } catch (auditError) {
    void logWarn(`Failed to persist application update activity: ${String(auditError)}`);
  }
}

export async function closeUpdate(update: Update): Promise<void> {
  if (activeUpdate === update) {
    activeUpdate = null;
    const release = (async () => {
      try {
        await update.close();
      } catch (error) {
        void logWarn(`Failed to release application update handle: ${String(error)}`);
      }
    })();
    closingUpdate = release;
    try {
      await release;
    } finally {
      if (closingUpdate === release) closingUpdate = null;
    }
    return;
  }
  try {
    await update.close();
  } catch (error) {
    void logWarn(`Failed to release application update handle: ${String(error)}`);
  }
}
