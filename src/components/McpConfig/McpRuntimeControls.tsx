import { useEffect } from 'react';
import { Alert, App, Button, Space, Typography } from 'antd';
import { PauseCircleOutlined, PlayCircleOutlined, ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { RuntimeInputPrompt } from '@/components/InputVariables/RuntimeInputPrompt';
import { useMcpStore } from '@/stores/mcpStore';
import { McpServerList } from './McpServerList';
import { useMcpRuntimeActions, type McpRuntimeAction } from './useMcpRuntimeActions';

const { Title } = Typography;

interface McpRuntimeControlsProps {
  instanceId: string;
  disabled?: boolean;
}

function runtimeSuccessMessage(action: McpRuntimeAction) {
  switch (action.kind) {
    case 'start':
      return { key: 'mcp.messages.started' as const, options: { name: action.name } };
    case 'startAll':
      return { key: 'mcp.messages.allStarted' as const };
    default:
      return null;
  }
}

export function McpRuntimeControls({ instanceId, disabled = false }: McpRuntimeControlsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    servers,
    loading,
    error,
    fetchServers,
    startServer,
    stopServer,
    startAll,
    stopAll,
  } = useMcpStore();

  useEffect(() => {
    void fetchServers(instanceId);
  }, [fetchServers, instanceId]);

  const runtimeActions = useMcpRuntimeActions({
    instanceId,
    startServer,
    startAll,
    onError: (errorMessage) => message.error(errorMessage),
    onSuccess: (action) => {
      const success = runtimeSuccessMessage(action);
      if (success) message.success(t(success.key, success.options));
    },
  });

  const handleStopAll = async () => {
    try {
      await stopAll(instanceId);
      message.success(t('mcp.messages.allStopped'));
    } catch (cause) {
      message.error(String(cause));
    }
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
          description={error}
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      <McpServerList
        servers={servers}
        mode="runtime"
        actionsDisabled={disabled}
        loading={loading}
        onStop={(bundleId) => stopServer(instanceId, bundleId)}
        onStart={(bundleId) => {
          const server = servers.find((candidate) => candidate.bundleId === bundleId);
          return runtimeActions.run({ kind: 'start', bundleId, name: server?.name ?? bundleId });
        }}
      />

      {runtimeActions.pending && (
        <RuntimeInputPrompt
          key={`${instanceId}:${runtimeActions.pending.error.input_id}`}
          instanceId={instanceId}
          error={runtimeActions.pending.error}
          onCancel={runtimeActions.cancel}
          onSubmitted={runtimeActions.retry}
        />
      )}
    </div>
  );
}
