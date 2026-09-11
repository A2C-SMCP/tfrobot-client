import { usePageActive, usePageAction } from '@/components/Navigation/pageActivityState';
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Alert, App, Button, Input, Space, Spin, Switch, Tag, Typography } from 'antd';
import { FolderOpenOutlined, UndoOutlined } from '@ant-design/icons';
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
}

const isWorkspaceLocked = (runtimeState: RuntimeState) => (
  runtimeState === 'starting' || runtimeState === 'running'
);

export function CommandLineToolSettings({ computerId }: CommandLineToolSettingsProps) {
  const action = usePageAction(computerId);
  const { message } = App.useApp();
  const pageActive = usePageActive();
  const { t } = useTranslation();
  const fetchInstances = useComputerStore((store) => store.fetchInstances);
  const runtimeRevision = useComputerStore((store) => (
    store.instances.find((instance) => instance.id === computerId)?.runtime.snapshot_revision
  ));
  const [state, setState] = useState<CommandLineToolState | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const loadSequence = useRef(0);

  const load = useCallback(async () => {
    const sequence = ++loadSequence.current;
    setLoading(true);
    try {
      const next = await invoke<CommandLineToolState>('get_command_line_tool_state', {
        computerId,
      });
      if (sequence === loadSequence.current) setState(next);
    } catch (error) {
      if (sequence === loadSequence.current) void message.error(String(error));
    } finally {
      if (sequence === loadSequence.current) setLoading(false);
    }
  }, [computerId, message]);

  useEffect(() => {
    if (pageActive) void load();
    return () => { loadSequence.current += 1; };
  }, [load, runtimeRevision, pageActive]);

  const update = async (policy: CommandLineToolPolicy) => {
    const current = action();
    ++loadSequence.current;
    setSaving(true);
    try {
      const next = await invoke<CommandLineToolState>('update_command_line_tool_policy', {
        request: { computerId, policy },
      });
      if (!current()) return;
      setState(next);
      await fetchInstances();
      if (current()) void message.success(t('computer.builtInTools.commandLine.saved'));
    } catch (error) {
      if (!current()) return;
      void message.error(String(error));
      await load();
    } finally {
      setSaving(false);
    }
  };

  const chooseWorkspace = async () => {
    if (
      !state
      || saving
      || isWorkspaceLocked(state.runtimeState)
    ) return;
    const current = action();
    const selected = await open({
      directory: true,
      multiple: false,
      title: t('computer.builtInTools.commandLine.chooseWorkspace'),
      defaultPath: state.effectiveWorkspace,
    });
    if (typeof selected === 'string' && current()) {
      const latest = await invoke<CommandLineToolState>('get_command_line_tool_state', {
        computerId,
      });
      if (!current()) return;
      setState(latest);
      if (!isWorkspaceLocked(latest.runtimeState)) {
        await update({ ...latest.policy, workspace_root: selected });
      }
    }
  };

  if (loading && !state) return <Spin />;
  if (!state) return null;

  const statusColor: Record<RuntimeState, string> = {
    disabled: 'default',
    pending: 'blue',
    starting: 'processing',
    running: 'success',
    error: 'error',
  };
  const workspaceLocked = isWorkspaceLocked(state.runtimeState);

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
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
        <Tag color={statusColor[state.runtimeState]}>
          {t(`computer.builtInTools.commandLine.status.${state.runtimeState}`)}
        </Tag>
      </Space>

      {!state.assetsAvailable && (
        <Alert
          type="error"
          showIcon
          message={t('computer.builtInTools.commandLine.assetsUnavailable')}
          description={state.error}
        />
      )}
      {state.runtimeState === 'pending' && (
        <Alert
          type="info"
          showIcon
          message={t('computer.builtInTools.commandLine.pendingDescription')}
        />
      )}
      {state.runtimeState === 'error' && state.assetsAvailable && state.error && (
        <Alert type="error" showIcon message={state.error} />
      )}

      <Space direction="vertical" size={4} style={{ width: '100%' }}>
        <Text strong>{t('computer.builtInTools.commandLine.workspace')}</Text>
        <Input
          value={state.effectiveWorkspace}
          readOnly
          disabled={workspaceLocked}
          title={state.effectiveWorkspace}
          aria-label={t('computer.builtInTools.commandLine.workspace')}
        />
        <Text type="secondary">
          {workspaceLocked
            ? t('computer.builtInTools.commandLine.workspaceLocked')
            : state.policy.workspace_root
              ? t('computer.builtInTools.commandLine.customWorkspace')
              : t('computer.builtInTools.commandLine.defaultWorkspace')}
        </Text>
        <Space>
          <Button
            icon={<FolderOpenOutlined />}
            disabled={saving || workspaceLocked}
            onClick={() => void chooseWorkspace()}
            aria-label={t('computer.builtInTools.commandLine.chooseWorkspace')}
          >
            {t('computer.builtInTools.commandLine.chooseWorkspace')}
          </Button>
          {state.policy.workspace_root && (
            <Button
              icon={<UndoOutlined />}
              disabled={saving || workspaceLocked}
              onClick={() => void update({ ...state.policy, workspace_root: undefined })}
              aria-label={t('computer.builtInTools.commandLine.useDefaultWorkspace')}
            >
              {t('computer.builtInTools.commandLine.useDefaultWorkspace')}
            </Button>
          )}
        </Space>
      </Space>
    </Space>
  );
}
