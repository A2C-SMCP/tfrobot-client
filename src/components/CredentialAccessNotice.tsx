import { useEffect, useState } from 'react';
import { Alert, Button, App, Space } from 'antd';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';

interface PausedCredential { id: string; purpose: 'oauth' | 'input' | 'connection'; context?: { computerId?: string; resourceId: string } }

/** Event-driven recovery UI. Retrying enables one credential, never replays a mutation. */
export function CredentialAccessNotice() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [attempt, setAttempt] = useState(0);
  const [paused, setPaused] = useState<PausedCredential[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    let sequence = 0;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      const request = ++sequence;
      try {
        const pending = await invoke<PausedCredential[]>('list_paused_credentials');
        if (!disposed && request === sequence) { setPaused(pending ?? []); setError(null); }
      } catch (reason) {
        if (!disposed) setError(String(reason));
      }
    };
    void listen('credentials:access-changed', () => { void refresh(); }).then((stop) => {
      if (disposed) stop();
      else { unlisten = stop; void refresh(); }
    }).catch((reason) => { if (!disposed) setError(String(reason)); });
    return () => { disposed = true; unlisten?.(); };
  }, [attempt]);
  const retry = async (id: string) => {
    setBusy(id);
    try {
      await invoke('retry_credential_access', { id });
      setPaused((current) => current.filter((entry) => entry.id !== id));
      setError(null);
      message.info(t('permissions.ready'));
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(null); }
  };
  return <Space direction="vertical" style={{ width: '100%' }}>
    {error && <Alert type="error" message={error} action={<Button onClick={() => setAttempt((value) => value + 1)}>{t('common.refresh')}</Button>} />}
    {paused.map((entry) => <Alert key={entry.id} type="warning" showIcon
      message={`${t('permissions.title')}: ${t(`permissions.${entry.purpose}`)}${entry.context ? ` — ${[entry.context.computerId, entry.context.resourceId].filter(Boolean).join(' / ')}` : ''}`}
      description={t('permissions.description')}
      action={<Button loading={busy === entry.id} disabled={busy !== null}
        onClick={() => { void retry(entry.id); }}>{t('permissions.retry')}</Button>} />)}
  </Space>;
}
