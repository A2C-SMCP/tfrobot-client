import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import styles from './ComputerSettings.module.css';
import { ConfigSaveStatus } from './ConfigSaveStatus';
import { useConfigCommit } from './useConfigCommit';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { usePageActive, usePageAction } from '@/components/Navigation/pageActivityState';
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Alert, App, Button, Input, Space, Radio, Spin, Switch, Typography } from 'antd';
import { FolderOpenOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useComputerStore, type CommandLineToolPolicy } from '@/stores/computerStore';

const { Text } = Typography;

type RuntimeState = 'disabled' | 'pending' | 'starting' | 'running' | 'error';

interface CommandLineToolState {
  policy: CommandLineToolPolicy;
  effectiveWorkspace: string;
  runtimeState: RuntimeState;
  assetsAvailable: boolean;
  error?: string;
}

interface CommandLineToolSettingsProps {
  computerId: string;
  onOpenRuntime?: () => void;
}

const isWorkspaceLocked = (runtimeState: RuntimeState) => (
  runtimeState === 'starting' || runtimeState === 'running'
);

export function CommandLineToolSettings({ computerId, onOpenRuntime }: CommandLineToolSettingsProps) {
  const action = usePageAction(computerId);
  const { message } = App.useApp();
  const pageActive = usePageActive();
  const { t } = useTranslation();
  const fetchInstances = useComputerStore((store) => store.fetchInstances);
  const runtimeRevision = useComputerStore((store) => (
    store.instances.find((instance) => instance.id === computerId)?.runtime.snapshot_revision
  ));
  const [state, setState] = useNavigationState<CommandLineToolState | null>(`settings.commandLine.${computerId}`, null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const submission = useConfigCommit(`settings.commandLine.${computerId}.commit`, (policy: CommandLineToolPolicy) => invoke<CommandLineToolState>('update_command_line_tool_policy', { request: { computerId, policy } }));
  const saving = submission.state === 'saving';
  const failedPolicy = submission.state === 'error' ? submission.last : null;
  const { capture } = submission;
  const loadSequence = useRef(0);
  const workspaceSession = useRef(0);
  const workspaceInFlight = useRef(false);
  const [checkingWorkspace, setCheckingWorkspace] = useState(false);
  const workspaceBusy = saving || checkingWorkspace;
  const [workspaceDraft, setWorkspaceDraft] = useNavigationState<{ custom: boolean; path: string } | null>(`settings.commandLine.workspace.${computerId}`, null);

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    const current = capture();
    setLoading(true);
    setLoadError(false);
    try {
      const next = await invoke<CommandLineToolState>('get_command_line_tool_state', {
        computerId,
      });
      if (sequence === loadSequence.current && current()) setState(next);
    } catch (error) {
      if (sequence === loadSequence.current) { setLoadError(true); void message.error(String(error)); }
    } finally {
      if (sequence === loadSequence.current) setLoading(false);
    }
  }, [computerId, message, capture, setState]);

  useEffect(() => {
    if (pageActive) void load();
    return () => { loadSequence.current += 1; };
  }, [load, runtimeRevision, pageActive]);

  const update = async (policy: CommandLineToolPolicy) => {
    const current = action();
    ++loadSequence.current;
    const acknowledged = await submission.commit(policy, undefined, (next) => { setState(next); setLoading(false); });
    if (!current()) return false;
    if (acknowledged) {
      await fetchInstances();
      if (current()) void message.success(t('computer.builtInTools.commandLine.saved'));
    } else if (submission.getError()) {
      void message.error(String(submission.getError()));
      await load();
    }
    return acknowledged;
  };

  const chooseWorkspace = async () => {
    if (!state || !workspaceDraft || workspaceInFlight.current || saving) return;
    const current = action();
    const session = workspaceSession.current;
    try {
      const selected = await open({ directory: true, multiple: false, title: t('computer.builtInTools.commandLine.chooseWorkspace'), defaultPath: state.effectiveWorkspace });
      if (typeof selected === 'string' && current() && session === workspaceSession.current) setWorkspaceDraft({ custom: true, path: selected });
    } catch (error) { if (current()) message.error(String(error)); }
  };
  const saveWorkspace = async () => {
    if (!workspaceDraft || (workspaceDraft.custom && !workspaceDraft.path.trim()) || workspaceInFlight.current || saving) return;
    const current = action();
    const session = workspaceSession.current;
    workspaceInFlight.current = true;
    setCheckingWorkspace(true);
    try {
      const latest = await invoke<CommandLineToolState>('get_command_line_tool_state', { computerId });
      if (!current() || session !== workspaceSession.current) return;
      setState(latest);
      if (isWorkspaceLocked(latest.runtimeState)) { message.warning(t('computer.builtInTools.commandLine.workspaceLocked')); return; }
      const saved = await update({ ...latest.policy, workspace_root: workspaceDraft.custom ? workspaceDraft.path.trim() : undefined });
      if (saved && current() && session === workspaceSession.current) setWorkspaceDraft(null);
    } catch (error) { if (current()) message.error(String(error)); }
    finally { workspaceInFlight.current = false; setCheckingWorkspace(false); }
  };

  if (loading && !state) return <Spin />;
  if (!state) return <Alert type="error" showIcon message={t('computer.redesign.loadCommandPolicyFailed')} action={<Button onClick={() => void load()}>{t('common.retry')}</Button>} />;

  const workspaceLocked = isWorkspaceLocked(state.runtimeState);
  const workspaceChanged = workspaceDraft !== null && (workspaceDraft.custom ? workspaceDraft.path.trim() : undefined) !== state.policy.workspace_root;

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {loadError && <Alert type="warning" showIcon message={t('computer.redesign.savedPolicyRefreshFailed')} action={<Button onClick={() => void load()}>{t('common.retry')}</Button>} />}
      <ConfigSaveStatus state={submission.state} onRetry={() => failedPolicy && update(failedPolicy)} />
      <Space align="start" style={{ justifyContent: 'space-between', width: '100%' }}>
        <Space align="start">
          <Switch
            checked={state.policy.enabled}
            loading={saving}
            disabled={!state.assetsAvailable && !state.policy.enabled}
            onChange={(enabled) => void update({ ...state.policy, enabled })}
            aria-label={t('computer.builtInTools.commandLine.enabled')}
          />
          <Space direction="vertical" size={0}>
            <Text strong>{t('computer.builtInTools.commandLine.enabled')}</Text>
            <Text type="secondary">
              {t('computer.builtInTools.commandLine.enabledDescription')}
            </Text>
          </Space>
        </Space>
        {onOpenRuntime && <Button type="link" onClick={onOpenRuntime}>{t('computer.redesign.viewRuntime')}</Button>}
      </Space>

      {!state.assetsAvailable && (
        <Alert
          type="error"
          showIcon
          message={t('computer.builtInTools.commandLine.assetsUnavailable')}
          description={state.error}
          action={(
            <Button size="small" onClick={() => { void load(); }}>
              {t('common.refresh')}
            </Button>
          )}
        />
      )}
      {state.runtimeState === 'error' && state.assetsAvailable && state.error && (
        <Alert
          type="error"
          showIcon
          message={state.error}
          action={(
            <Space>
              <Button size="small" loading={loading} onClick={() => { void load(); }}>
                {t('common.refresh')}
              </Button>
              {onOpenRuntime && (
                <Button size="small" onClick={onOpenRuntime}>
                  {t('computer.workbench.sections.runtime')}
                </Button>
              )}
            </Space>
          )}
        />
      )}

      <Space direction="vertical" size={8} style={{ width: '100%' }}>
        <Text strong>{t('computer.builtInTools.commandLine.workspace')}</Text>
        <Text code copyable className={styles.storagePath}>{state.effectiveWorkspace}</Text>
        <Text type="secondary">{t(workspaceLocked ? 'computer.builtInTools.commandLine.workspaceLocked' : state.policy.workspace_root ? 'computer.builtInTools.commandLine.customWorkspace' : 'computer.builtInTools.commandLine.defaultWorkspace')}</Text>
        <Button disabled={saving || workspaceLocked} onClick={() => { workspaceSession.current += 1; setWorkspaceDraft({ custom: Boolean(state.policy.workspace_root), path: state.policy.workspace_root ?? state.effectiveWorkspace }); }}>{t('computer.redesign.editWorkspace')}</Button>
      </Space>
      <Modal title={t('computer.redesign.editWorkspace')} open={workspaceDraft !== null} okText={t('common.save')} cancelText={t('common.cancel')}
        confirmLoading={workspaceBusy} closable={!workspaceBusy} maskClosable={!workspaceBusy} cancelButtonProps={{ disabled: workspaceBusy }}
        okButtonProps={{ 'aria-label': t('common.save'), disabled: workspaceBusy || !workspaceChanged || !workspaceDraft || (workspaceDraft.custom && !workspaceDraft.path.trim()) || workspaceLocked }}
        onOk={() => void saveWorkspace()} onCancel={() => { if (!workspaceInFlight.current && !saving) { workspaceSession.current += 1; setWorkspaceDraft(null); submission.reset(); } }}>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <Text type="secondary">{t('computer.redesign.workspaceImpact')}</Text>
          <Radio.Group value={workspaceDraft?.custom ?? false} disabled={workspaceBusy} onChange={(event) => workspaceDraft && setWorkspaceDraft({ ...workspaceDraft, custom: event.target.value })}
            options={[{ value: false, label: t('computer.storage.default') }, { value: true, label: t('computer.storage.custom') }]} />
          {workspaceDraft?.custom && <Space.Compact style={{ width: '100%' }}>
            <Input aria-label={t('computer.builtInTools.commandLine.workspace')} value={workspaceDraft.path} disabled={workspaceBusy} onChange={(event) => setWorkspaceDraft({ custom: true, path: event.target.value })} />
            <Button disabled={workspaceBusy} icon={<FolderOpenOutlined />} aria-label={t('computer.builtInTools.commandLine.chooseWorkspace')} onClick={() => void chooseWorkspace()} />
          </Space.Compact>}
          <ConfigSaveStatus state={workspaceBusy ? 'saving' : submission.state === 'error' ? 'error' : workspaceChanged ? 'dirty' : 'saved'} />
        </Space>
      </Modal>
    </Space>
  );
}
