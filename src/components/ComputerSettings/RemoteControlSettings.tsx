import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
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
  message,
} from 'antd';
import { useTranslation } from 'react-i18next';
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
  const { instances, fetchInstances } = useComputerStore();
  const [catalog, setCatalog] = useState<ClientControlTool[]>([]);
  const [policy, setPolicy] = useState<RemoteControlPolicy>(
    instance.remoteControl ?? DEFAULT_POLICY,
  );
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    Promise.all([
      invoke<ClientControlTool[]>('get_client_control_catalog'),
      invoke<RemoteControlPolicy>('get_remote_control_policy', { computerId: instance.id }),
    ]).then(([nextCatalog, nextPolicy]) => {
      if (!cancelled) {
        setCatalog(nextCatalog);
        setPolicy(nextPolicy);
      }
    }).catch((error) => {
      if (!cancelled) void message.error(String(error));
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => { cancelled = true; };
  }, [instance.id]);

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
    setSaving(true);
    try {
      const updated = await invoke<RemoteControlPolicy>('update_remote_control_policy', {
        request: { computerId: instance.id, policy },
      });
      setPolicy(updated);
      await fetchInstances();
      void message.success(t('computer.remoteControl.saved'));
    } catch (error) {
      void message.error(String(error));
    } finally {
      setSaving(false);
    }
  };

  if (loading) return <Spin />;

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Alert
        type="warning"
        showIcon
        message={t('computer.remoteControl.securityTitle')}
        description={t('computer.remoteControl.securityDescription')}
      />

      <Space align="start">
        <Switch
          checked={policy.enabled}
          onChange={(enabled) => setPolicy((current) => ({ ...current, enabled }))}
          aria-label={t('computer.remoteControl.enabled')}
        />
        <Space direction="vertical" size={0}>
          <Text strong>{t('computer.remoteControl.enabled')}</Text>
          <Text type="secondary">{t('computer.remoteControl.enabledDescription')}</Text>
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

      <Alert
        type="info"
        showIcon
        message={t('computer.remoteControl.selfProtection')}
      />
      <Button type="primary" loading={saving} onClick={() => void save()}>
        {t('common.save')}
      </Button>
    </Space>
  );
}
