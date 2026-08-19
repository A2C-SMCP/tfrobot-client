import { useEffect, useRef, useState } from 'react';
import { Alert, App, Button, Space, Typography } from 'antd';
import { PauseCircleOutlined, PlayCircleOutlined, ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  useMcpStore,
  type McpBatchOperationResult,
  type McpServerManagedBy,
} from '@/stores/mcpStore';
import type { ComputerRuntimeActionCapability } from '@/stores/runtimeSnapshot';
import { McpServerList } from './McpServerList';
import { useMcpRuntimeActions, type McpRuntimeAction } from './useMcpRuntimeActions';

const { Title } = Typography;
type PluginMcpServerOwner = Extract<McpServerManagedBy, { type: 'plugin' }>;

interface McpRuntimeControlsProps {
  instanceId: string;
  capability: ComputerRuntimeActionCapability;
  onStartRuntime?: () => void;
  onRestartRuntime?: () => void;
  onOpenPlugin?: (owner: PluginMcpServerOwner) => void;
}

function runtimeSuccessMessage(action: McpRuntimeAction) {
  switch (action.kind) {
    case 'start':
      return { key: 'mcp.messages.started' as const, options: { name: action.name } };
    case 'retry':
      return { key: 'mcp.messages.reconnected' as const, options: { name: action.name } };
    default:
      return null;
  }
}

export function McpRuntimeControls({
  instanceId,
  capability,
  onStartRuntime,
  onRestartRuntime,
  onOpenPlugin,
}: McpRuntimeControlsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [batchFeedback, setBatchFeedback] = useState<{
    action: 'start' | 'stop';
    result: McpBatchOperationResult;
  } | null>(null);
  const activeInstanceRef = useRef<string | null>(instanceId);
  activeInstanceRef.current = instanceId;
  const {
    servers,
    loading,
    error,
    fetchServers,
    startServer,
    stopServer,
    startAll,
    stopAll,
    authorizeServer,
    cancelAuthorization,
    clearAuthorization,
  } = useMcpStore();

  useEffect(() => {
    void fetchServers(instanceId);
    setBatchFeedback(null);
    return () => {
      if (activeInstanceRef.current === instanceId) {
        activeInstanceRef.current = null;
      }
    };
  }, [fetchServers, instanceId]);

  const disabled = !capability.enabled;
  const disabledReason = capability.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${capability.disabled_reason}`)
    : undefined;
  const runtimeActions = useMcpRuntimeActions({
    instanceId,
    startServer,
    stopServer,
    startAll,
    onBatchResult: (result) => setBatchFeedback({ action: 'start', result }),
    onError: () => message.error(t('mcp.messages.operationFailed')),
    onSuccess: (action) => {
      const success = runtimeSuccessMessage(action);
      if (success) message.success(t(success.key, success.options));
    },
  });

  const handleStopAll = async () => {
    const actionInstanceId = instanceId;
    try {
      const result = await stopAll(actionInstanceId);
      if (activeInstanceRef.current !== actionInstanceId) return;
      setBatchFeedback({ action: 'stop', result });
    } catch {
      if (activeInstanceRef.current !== actionInstanceId) return;
      message.error(t('mcp.messages.operationFailed'));
    }
  };

  const handleStopServer = async (bundleId: string, name: string) => {
    const actionInstanceId = instanceId;
    try {
      await stopServer(actionInstanceId, bundleId);
      if (activeInstanceRef.current !== actionInstanceId) return;
      message.success(t('mcp.messages.stopped', { name }));
    } catch {
      if (activeInstanceRef.current !== actionInstanceId) return;
      message.error(t('mcp.messages.operationFailed'));
    }
  };

  const runAuthorizationAction = async (
    action: (instanceId: string, bundleId: string) => Promise<void>,
    bundleId: string,
  ) => {
    const actionInstanceId = instanceId;
    try {
      await action(actionInstanceId, bundleId);
    } catch {
      if (activeInstanceRef.current === actionInstanceId) {
        message.error(t('mcp.messages.operationFailed'));
      }
    }
  };

  const renderDisabledGuidance = () => {
    if (!disabledReason) return null;
    const reason = capability.disabled_reason;
    const nextAction = reason === 'not_running' && onStartRuntime
      ? (
          <Button size="small" type="primary" onClick={onStartRuntime}>
            {t('mcp.disabledGuidance.startRuntime')}
          </Button>
        )
      : reason === 'degraded' && onRestartRuntime
        ? (
            <Button size="small" onClick={onRestartRuntime}>
              {t('mcp.disabledGuidance.restartRuntime')}
            </Button>
          )
        : (
            <Typography.Text>
              {t('mcp.disabledGuidance.refreshAfterTransition')}
            </Typography.Text>
          );
    return (
      <Alert
        type="info"
        showIcon
        message={t('mcp.disabledGuidance.title')}
        description={(
          <Space direction="vertical" size={8}>
            <Typography.Text>{disabledReason}</Typography.Text>
            {nextAction}
          </Space>
        )}
        style={{ marginBottom: 16 }}
      />
    );
  };

  const renderBatchFeedback = () => {
    if (!batchFeedback) return null;
    const { action, result } = batchFeedback;
    return (
      <Alert
        type={result.failures.length > 0 ? 'warning' : 'success'}
        showIcon
        closable
        onClose={() => setBatchFeedback(null)}
        message={t(`mcp.batch.${action}Summary`, {
          candidates: result.candidate_count,
          actual: result.actual_operation_count,
          unchanged: result.unchanged_count,
          excluded: result.excluded_plugin_owned_count,
          failed: result.failures.length,
        })}
        description={result.failures.length > 0 && (
          <ul style={{ margin: 0, paddingInlineStart: 20 }}>
            {result.failures.map((failure) => (
              <li key={failure.bundleId}>
                {t('mcp.batch.failure', {
                  name: failure.name,
                  bundleId: failure.bundleId,
                })}
              </li>
            ))}
          </ul>
        )}
        style={{ marginBottom: 16 }}
      />
    );
  };

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('mcp.runtimeTitle')}</Title>
        <Space>
          <Button
            icon={<ReloadOutlined />}
            onClick={() => fetchServers(instanceId)}
            loading={loading}
          >
            {t('common.refresh')}
          </Button>
          <Button
            icon={<PlayCircleOutlined />}
            onClick={() => runtimeActions.run({ kind: 'startAll' })}
            disabled={disabled}
            loading={loading}
          >
            {t('mcp.startAll')}
          </Button>
          <Button
            icon={<PauseCircleOutlined />}
            onClick={handleStopAll}
            disabled={disabled}
            loading={loading}
          >
            {t('mcp.stopAll')}
          </Button>
        </Space>
      </div>

      {error && (
        <Alert
          message={t('common.error')}
          description={t('mcp.messages.statusLoadFailed')}
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      {renderDisabledGuidance()}
      {renderBatchFeedback()}

      <McpServerList
        servers={servers}
        actionsDisabled={disabled}
        actionsDisabledReason={disabledReason}
        loading={loading}
        onOpenPlugin={onOpenPlugin}
        onAuthorize={(bundleId) => runAuthorizationAction(authorizeServer, bundleId)}
        onCancelAuthorization={(bundleId) => runAuthorizationAction(cancelAuthorization, bundleId)}
        onClearAuthorization={(bundleId) => runAuthorizationAction(clearAuthorization, bundleId)}
        onStop={(bundleId, name) => handleStopServer(bundleId, name)}
        onRetry={(bundleId, name) => runtimeActions.run({ kind: 'retry', bundleId, name })}
        onStart={(bundleId) => {
          const server = servers.find((candidate) => candidate.bundleId === bundleId);
          return runtimeActions.run({ kind: 'start', bundleId, name: server?.name ?? bundleId });
        }}
      />

    </div>
  );
}
