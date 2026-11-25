import type { PluginSettingsTarget } from './tabs';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Alert, Button, Collapse, Space, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useConfigAdoptionStore, type ConfigAdoptionChange } from '@/stores/configAdoptionStore';
import type { ComputerInstance } from '@/stores/computerStore';
import { useMcpStore } from '@/stores/mcpStore';
import { formatRuntimeActionError } from '@/utils/runtimeActionError';

interface Props {
  instance: ComputerInstance;
  compact?: boolean;
  quiet?: boolean;
  summaryOnly?: boolean;
  onOpenRuntime?: () => void;
  onRestart?: () => void;
  onOpenPlugin?: (owner: PluginSettingsTarget) => void;
}
export function ConfigAdoptionStatus({ instance, compact, quiet, summaryOnly, onOpenRuntime, onRestart, onOpenPlugin }: Props) {
  const { t } = useTranslation();
  const status = useConfigAdoptionStore((state) => state.statuses[instance.id]);
  const checking = useConfigAdoptionStore((state) => state.loading[instance.id]);
  const failed = useConfigAdoptionStore((state) => state.errors[instance.id]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  useEffect(() => useConfigAdoptionStore.getState().watch(instance.id), [instance.id]);
  const state = checking ? 'checking' : failed ? 'unknown' : status?.state ?? 'checking';
  const run = async (change: ConfigAdoptionChange) => {
    if (busy) return;
    setBusy(true); setError(undefined);
    try {
      if (change.action === 'manage_plugin' && change.owner?.type === 'plugin') onOpenPlugin?.(change.owner);
      else if (change.action === 'stop_mcp') await useMcpStore.getState().stopServer(instance.id, change.resource_id);
      else if (change.action === 'start_mcp') await useMcpStore.getState().startServer(instance.id, change.resource_id);
      else if (change.action === 'restart_mcp') await invoke('restart_mcp_server_with_latest_config', { instanceId: instance.id, bundleId: change.resource_id });
      else onRestart?.();
    } catch (cause) { setError(formatRuntimeActionError(cause)); }
    finally { setBusy(false); useConfigAdoptionStore.getState().invalidate(instance.id); }
  };
  if (summaryOnly) {
    if (state !== 'pending' && state !== 'unknown') return null;
    return <Space wrap role="status" style={{ marginBottom: 20 }}>
      <Typography.Text type={state === 'pending' ? 'warning' : 'secondary'}>{t(`computer.adoption.${state}`, { count: status?.changes.length ?? 0 })}</Typography.Text>
      {onOpenRuntime && <Button type="link" onClick={onOpenRuntime}>{t('computer.redesign.viewAdoption')}</Button>}
      {failed && <Button size="small" onClick={() => useConfigAdoptionStore.getState().invalidate(instance.id)}>{t('computer.adoption.retry')}</Button>}
    </Space>;
  }
  if (compact) return state === 'pending' || state === 'unknown' ? <Button type="link" size="small" onClick={onOpenRuntime}>{t(`computer.adoption.${state}`, { count: status?.changes.length ?? 0 })}</Button> : <Typography.Text type="secondary" role="status">{t(`computer.adoption.compact.${state}`)}</Typography.Text>;
  if (quiet && (state === 'checking' || state === 'current' || state === 'waiting_start') && !status?.changes.length && !status?.unknown_resources.length) {
    return <Typography.Paragraph type="secondary" role="status" style={{ marginBottom: 24 }}>{t(`computer.adoption.settings.${state}`)}</Typography.Paragraph>;
  }
  return <Space direction="vertical" style={{ width: '100%', marginBottom: 16 }}>
    <Alert type={state === 'pending' ? 'warning' : 'info'} showIcon
      message={<span role="status">{t(`computer.adoption.${state}`, { count: status?.changes.length ?? 0 })}</span>}
      description={state === 'current' ? t('computer.adoption.healthSeparate') : state === 'waiting_start' ? t('computer.adoption.waitingDescription') : undefined}
      action={onOpenRuntime && state !== 'current' && <Button size="small" onClick={onOpenRuntime}>{t('computer.adoption.openRuntime')}</Button>} />
    {!failed && !!status?.changes.length && <Collapse items={[{ key: 'changes', label: t('computer.adoption.viewChanges'), children: <Space direction="vertical" style={{ width: '100%' }}>
      {status.changes.map((change) => <Space key={`${change.resource_id}:${change.reason}`} direction="vertical">
        <Typography.Text strong>{change.resource_id}</Typography.Text>
        <Typography.Text>{t(`computer.adoption.reasons.${change.reason}`)}</Typography.Text>
        {change.current_value != null && <Typography.Text>{change.current_value} → {change.saved_value}</Typography.Text>}
        {change.action === 'manage_plugin' && change.owner?.type === 'plugin' ? <Button onClick={() => change.owner?.type === 'plugin' && onOpenPlugin?.(change.owner)}>{t('computer.adoption.actions.manage_plugin')}</Button> : onRestart ? <Button loading={busy} disabled={change.action === 'restart_computer' ? !instance.runtime.actions.restart.enabled : !instance.runtime.actions.manage_mcp.enabled}
          onClick={() => void run(change)}>{t(`computer.adoption.actions.${change.action}`)}</Button>
          : <Typography.Text type="secondary">{t(`computer.adoption.actions.${change.action}`)}</Typography.Text>}
      </Space>)}
    </Space> }]} />}
    {!!status?.unknown_resources.length && <Typography.Text type="secondary">{t('computer.adoption.inputsUnknown')}</Typography.Text>}
    {status?.unknown_resources.map((resource) => {
      const owner = status.plugin_owners?.[resource];
      return <Space key={resource} wrap><Typography.Text code>{resource}</Typography.Text>{owner && onOpenPlugin && <Button onClick={() => onOpenPlugin(owner)}>{t('computer.adoption.openPlugin', { name: owner.plugin })}</Button>}</Space>;
    })}
    {error && <Alert type="error" showIcon message={error} />}
    {failed && <Button size="small" onClick={() => useConfigAdoptionStore.getState().invalidate(instance.id)}>{t('computer.adoption.retry')}</Button>}
  </Space>;
}
