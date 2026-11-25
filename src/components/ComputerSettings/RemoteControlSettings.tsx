import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import { ConfigSaveStatus } from './ConfigSaveStatus';
import { useConfigCommit } from './useConfigCommit';
import { usePageActive, usePageAction } from '@/components/Navigation/pageActivityState';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  App,
  Alert,
  Button,
  Checkbox,
  Divider,
  Radio,
  Select,
  Space,
  Spin,
  Switch,
  Typography,
  Tag,
} from 'antd';
import { useTranslation } from 'react-i18next';
import { NoticeBar } from '@/components/common/NoticeBar';
import { useNoticeLifecycle } from '@/components/common/useNoticeLifecycle';
import {
  useComputerStore,
  type ComputerInstance,
  type RemoteControlPolicy,
} from '@/stores/computerStore';

const { Text, Title } = Typography;

interface ClientControlTool {
  id: string;
  group: string;
  risk: 'read_only' | 'mutating' | 'sensitive_write' | 'destructive';
  target: 'discovery' | 'optional' | 'required';
}

const DEFAULT_POLICY: RemoteControlPolicy = {
  enabled: false,
  tool_scope: { mode: 'all' },
  target_scope: { mode: 'self_only' },
};

interface RemoteControlSettingsProps {
  instance: ComputerInstance;
}

export function RemoteControlSettings({ instance }: RemoteControlSettingsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const active = usePageActive();
  const action = usePageAction(instance.id);
  const { instances, fetchInstances } = useComputerStore();
  const [catalog, setCatalog] = useState<ClientControlTool[]>([]);
  const [draftPolicy, setDraftPolicy] = useNavigationState<RemoteControlPolicy | null>(`settings.remoteControl.${instance.id}`, null);
  const [serverPolicy, setServerPolicy] = useNavigationState<RemoteControlPolicy>(`settings.remoteControl.saved.${instance.id}`, instance.remoteControl ?? DEFAULT_POLICY);
  const submission = useConfigCommit(`settings.remoteControl.${instance.id}.commit`, (policy: RemoteControlPolicy) => invoke<RemoteControlPolicy>('update_remote_control_policy', { request: { computerId: instance.id, policy } }));
  const { capture } = submission;
  const [editorOpen, setEditorOpen] = useNavigationState(`settings.remoteControl.editor.${instance.id}`, false);
  const policy = draftPolicy ?? serverPolicy;
  const setPolicy: React.Dispatch<React.SetStateAction<RemoteControlPolicy>> = (next) => { submission.dirty(); setDraftPolicy(typeof next === 'function' ? next(policy) : next); };
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [loadRevision, setLoadRevision] = useState(0);
  const saving = submission.state === 'saving';
  const loadSequence = useRef(0);
  useEffect(() => { setLoaded(false); setLoading(true); }, [instance.id]);
  const securityNotice = useNoticeLifecycle('remote-control-security', editorOpen && !(loading && !loaded));

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const sequence = ++loadSequence.current;
    const current = capture();
    setLoading(true);
    setLoadError(false);
    Promise.all([
      invoke<ClientControlTool[]>('get_client_control_catalog'),
      invoke<RemoteControlPolicy>('get_remote_control_policy', { computerId: instance.id }),
    ]).then(([nextCatalog, nextPolicy]) => {
      if (!cancelled && sequence === loadSequence.current) {
        setLoaded(true);
        setCatalog(nextCatalog);
        if (current()) setServerPolicy(nextPolicy);
      }
    }).catch((error) => {
      if (!cancelled && sequence === loadSequence.current) { setLoadError(true); void message.error(String(error)); }
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => { cancelled = true; };
  }, [active, instance.id, message, capture, setServerPolicy, loadRevision]);

  const grouped = useMemo(() => {
    const groups = new Map<string, ClientControlTool[]>();
    for (const tool of catalog) {
      groups.set(tool.group, [...(groups.get(tool.group) ?? []), tool]);
    }
    return groups;
  }, [catalog]);
  const selectedTools = policy.tool_scope.mode === 'custom' ? policy.tool_scope.tools : [];
  const selectedTargets = policy.target_scope.mode === 'custom' ? policy.target_scope.targets : [];

  const save = async () => {
    const current = action();
    ++loadSequence.current;
    const acknowledged = await submission.commit(policy, () => setDraftPolicy(null), (updated) => setServerPolicy(updated));
    if (acknowledged && current()) {
      setEditorOpen(false);
      await fetchInstances();
      if (current()) void message.success(t('computer.remoteControl.saved'));
    } else if (current() && submission.getError()) void message.error(String(submission.getError()));
  };

  if (!loaded) return loadError ? <Alert type="error" showIcon message={t('computer.redesign.loadAuthorizationFailed')} action={<Button onClick={() => setLoadRevision((revision) => revision + 1)}>{t('common.retry')}</Button>} /> : <Spin />;

  return (
    <>
      {loadError && <Alert type="warning" showIcon message={t('computer.redesign.savedPolicyRefreshFailed')} action={<Button onClick={() => setLoadRevision((revision) => revision + 1)}>{t('common.retry')}</Button>} />}
      <Space direction="vertical" size={12} style={{ width: '100%' }}>
        <Space wrap><Tag>{t(serverPolicy.enabled ? 'computer.redesign.policyEnabled' : 'computer.redesign.policyDisabled')}</Tag>
          <Text>{t(serverPolicy.tool_scope.mode === 'all' ? 'computer.remoteControl.allTools' : 'computer.redesign.selectedTools', { count: serverPolicy.tool_scope.mode === 'custom' ? serverPolicy.tool_scope.tools.length : 0 })}</Text>
          <Text>{t(serverPolicy.target_scope.mode === 'self_only' ? 'computer.remoteControl.selfOnly' : serverPolicy.target_scope.mode === 'all' ? 'computer.remoteControl.allComputers' : 'computer.redesign.selectedTargets', { count: serverPolicy.target_scope.mode === 'custom' ? serverPolicy.target_scope.targets.length : 0 })}</Text>
        </Space>
        <Text type="secondary">{t('computer.remoteControl.securitySummary')}</Text>
        <Button onClick={() => setEditorOpen(true)}>{t('computer.redesign.editAuthorization')}</Button>
        {!editorOpen && submission.state !== 'saved' && <ConfigSaveStatus state={submission.state} />}
      </Space>
      <Modal title={t('computer.redesign.editAuthorization')} open={editorOpen} footer={null} onCancel={() => { if (!saving) { setDraftPolicy(null); submission.reset(); setEditorOpen(false); } }} closable={!saving} maskClosable={!saving}>
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
      {securityNotice.visible && (
        <NoticeBar
          tone="warning"
          dismiss={{
            label: t('common.dismissNotice'),
            onClick: securityNotice.dismiss,
          }}
        >
          <Text strong>{t('computer.remoteControl.securityTitle')}</Text>{' '}
          <Text type="secondary">{t('computer.remoteControl.securityDescription')}</Text>
        </NoticeBar>
      )}

      <Space align="start">
        <Switch
          checked={policy.enabled}
          onChange={(enabled) => setPolicy((current) => ({ ...current, enabled }))}
          aria-label={t('computer.remoteControl.enabled')}
        />
        <Space direction="vertical" size={0}>
          <Text strong>{t('computer.remoteControl.enabled')}</Text>
          <Text type="secondary">{t('computer.remoteControl.enabledDescription')}</Text>
          {/* Dismissing the warning must not make the safety intent unrecoverable. */}
          {!securityNotice.visible && (
            <Text type="secondary">{t('computer.remoteControl.securitySummary')}</Text>
          )}
        </Space>
      </Space>

      <div>
        <Title level={5}>{t('computer.remoteControl.toolScope')}</Title>
        <Radio.Group
          value={policy.tool_scope.mode}
          onChange={(event) => setPolicy((current) => ({
            ...current,
            tool_scope: event.target.value === 'all'
              ? { mode: 'all' }
              : { mode: 'custom', tools: selectedTools },
          }))}
          options={[
            { value: 'all', label: t('computer.remoteControl.allTools') },
            { value: 'custom', label: t('computer.remoteControl.customTools') },
          ]}
        />
        {policy.tool_scope.mode === 'custom' && (
          <Space direction="vertical" style={{ width: '100%', marginTop: 16 }}>
            {[...grouped.entries()].map(([group, tools]) => (
              <div key={group}>
                <Text strong>{t(`computer.remoteControl.groups.${group}`)}</Text>
                <Checkbox.Group
                  style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', marginTop: 8 }}
                  value={selectedTools}
                  options={tools.map((tool) => ({
                    value: tool.id,
                    label: `${tool.id}${tool.risk === 'destructive' ? ' ⚠' : ''}`,
                  }))}
                  onChange={(tools) => setPolicy((current) => ({
                    ...current,
                    tool_scope: { mode: 'custom', tools: tools.map(String) },
                  }))}
                />
                <Divider style={{ margin: '12px 0' }} />
              </div>
            ))}
          </Space>
        )}
      </div>

      <div>
        <Title level={5}>{t('computer.remoteControl.targetScope')}</Title>
        <Radio.Group
          value={policy.target_scope.mode}
          onChange={(event) => setPolicy((current) => ({
            ...current,
            target_scope: event.target.value === 'self_only'
              ? { mode: 'self_only' }
              : event.target.value === 'all'
                ? { mode: 'all' }
                : { mode: 'custom', targets: selectedTargets },
          }))}
          options={[
            { value: 'self_only', label: t('computer.remoteControl.selfOnly') },
            { value: 'all', label: t('computer.remoteControl.allComputers') },
            { value: 'custom', label: t('computer.remoteControl.customComputers') },
          ]}
        />
        {policy.target_scope.mode === 'custom' && (
          <Select
            mode="multiple"
            style={{ width: '100%', marginTop: 16 }}
            value={selectedTargets}
            options={instances.map((item) => ({ value: item.id, label: `${item.name} (${item.id})` }))}
            onChange={(targets) => setPolicy((current) => ({
              ...current,
              target_scope: { mode: 'custom', targets },
            }))}
          />
        )}
      </div>

      <Text type="secondary">{t('computer.remoteControl.selfProtection')}</Text>
      <ConfigSaveStatus state={submission.state === 'saved' && draftPolicy ? 'dirty' : submission.state} />
      <Button disabled={saving} onClick={() => { setDraftPolicy(null); submission.reset(); setEditorOpen(false); }}>{t('common.cancel')}</Button>
      <Button type="primary" disabled={!draftPolicy} loading={saving} onClick={() => void save()}>
        {t('common.save')}
      </Button>
      </Space>
      </Modal>
    </>
  );
}
