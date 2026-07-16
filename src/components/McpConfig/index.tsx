import { useEffect, useState } from 'react';
import { App, Button, Space, Modal, Typography, Alert } from 'antd';
import {
  PlusOutlined,
  PlayCircleOutlined,
  PauseCircleOutlined,
  ReloadOutlined,
  ImportOutlined,
  ExportOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useMcpStore, type McpServerConfig } from '@/stores/mcpStore';
import { RuntimeInputPrompt } from '@/components/InputVariables/RuntimeInputPrompt';
import { McpServerList } from './McpServerList';
import { McpServerForm } from './McpServerForm';
import { useMcpRuntimeActions, type McpRuntimeAction } from './useMcpRuntimeActions';

const { Title } = Typography;

interface McpConfigProps {
  instanceId: string;
}

function runtimeActionSuccess(action: McpRuntimeAction) {
  switch (action.kind) {
    case 'add':
      return { key: 'mcp.messages.added' as const, closeForm: true };
    case 'update':
      return { key: 'mcp.messages.updated' as const, closeForm: true };
    case 'start':
      return {
        key: 'mcp.messages.started' as const,
        options: { name: action.name },
        closeForm: false,
      };
    case 'startAll':
      return { key: 'mcp.messages.allStarted' as const, closeForm: false };
  }
}

export function McpConfig({ instanceId }: McpConfigProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    servers,
    loading,
    error,
    fetchServers,
    addServer,
    updateServer,
    removeServer,
    startServer,
    stopServer,
    startAll,
    stopAll,
    getServerConfig,
    importConfig,
    exportConfig,
  } = useMcpStore();

  const [formVisible, setFormVisible] = useState(false);
  const [editingServer, setEditingServer] = useState<McpServerConfig | undefined>();

  useEffect(() => {
    fetchServers(instanceId);
  }, [fetchServers, instanceId]);

  const reportRuntimeActionSuccess = (action: McpRuntimeAction) => {
    const success = runtimeActionSuccess(action);
    message.success(t(success.key, success.options));
    if (success.closeForm) setFormVisible(false);
  };

  const runtimeActions = useMcpRuntimeActions({
    instanceId,
    addServer,
    updateServer,
    startServer,
    startAll,
    onError: (errorMessage) => message.error(errorMessage),
    onSuccess: reportRuntimeActionSuccess,
  });

  const handleAdd = () => {
    setEditingServer(undefined);
    setFormVisible(true);
  };

  const handleEdit = async (name: string) => {
    try {
      const config = await getServerConfig(instanceId, name);
      setEditingServer(config);
      setFormVisible(true);
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleFormSubmit = async (config: McpServerConfig) => {
    await runtimeActions.run(editingServer ? { kind: 'update', config } : { kind: 'add', config });
  };

  const handleStartAll = async () => {
    await runtimeActions.run({ kind: 'startAll' });
  };

  const handleStopAll = async () => {
    try {
      await stopAll(instanceId);
      message.success(t('mcp.messages.allStopped'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleImport = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        multiple: false,
      });
      if (path) {
        const result = await importConfig(instanceId, path as string);
        message.success(t('mcp.messages.importSuccess', { servers: result.servers_imported, inputs: result.inputs_imported }));
      }
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleExport = async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        defaultPath: 'mcp_config.json',
      });
      if (path) {
        await exportConfig(instanceId, path);
        message.success(t('mcp.messages.exportSuccess'));
      }
    } catch (e) {
      message.error(String(e));
    }
  };

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('mcp.servers')}</Title>
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
            onClick={handleStartAll}
            loading={loading}
          >
            {t('mcp.startAll')}
          </Button>
          <Button
            icon={<PauseCircleOutlined />}
            onClick={handleStopAll}
            loading={loading}
          >
            {t('mcp.stopAll')}
          </Button>
          <Button
            icon={<ImportOutlined />}
            onClick={handleImport}
          >
            {t('mcp.importConfig')}
          </Button>
          <Button
            icon={<ExportOutlined />}
            onClick={handleExport}
          >
            {t('mcp.exportConfig')}
          </Button>
          <Button
            type="primary"
            icon={<PlusOutlined />}
            onClick={handleAdd}
          >
            {t('mcp.addServer')}
          </Button>
        </Space>
      </div>

      {error && (
        <Alert
          message={t('common.error')}
          description={error}
          type="error"
          showIcon
          closable
          style={{ marginBottom: 16 }}
        />
      )}

      <McpServerList
        servers={servers}
        loading={loading}
        onStop={(name) => stopServer(instanceId, name)}
        onEdit={handleEdit}
        onRemove={(name) => removeServer(instanceId, name)}
        onStart={(name) => runtimeActions.run({ kind: 'start', name })}
      />

      <Modal
        title={editingServer ? t('mcp.editServer') : t('mcp.addServer')}
        open={formVisible}
        onCancel={() => setFormVisible(false)}
        footer={null}
        destroyOnHidden
        width={600}
      >
        <McpServerForm
          initialValues={editingServer}
          onSubmit={handleFormSubmit}
          onCancel={() => setFormVisible(false)}
          loading={loading}
        />
      </Modal>
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
