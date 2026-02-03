import { useEffect, useState } from 'react';
import { Button, Space, Modal, Typography, message, Alert } from 'antd';
import {
  PlusOutlined,
  PlayCircleOutlined,
  PauseCircleOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useMcpStore, type McpServerConfig } from '@/stores/mcpStore';
import { McpServerList } from './McpServerList';
import { McpServerForm } from './McpServerForm';

const { Title } = Typography;

export function McpConfig() {
  const { t } = useTranslation();
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
  } = useMcpStore();

  const [formVisible, setFormVisible] = useState(false);
  const [editingServer, setEditingServer] = useState<McpServerConfig | undefined>();

  useEffect(() => {
    fetchServers();
  }, [fetchServers]);

  const handleAdd = () => {
    setEditingServer(undefined);
    setFormVisible(true);
  };

  const handleEdit = (_name: string) => {
    // For now, we don't have the full config from the status
    // TODO: Add API to get full config by name
    message.info(t('mcp.messages.editNotImplemented'));
  };

  const handleFormSubmit = async (config: McpServerConfig) => {
    try {
      if (editingServer) {
        await updateServer(config);
        message.success(t('mcp.messages.updated'));
      } else {
        await addServer(config);
        message.success(t('mcp.messages.added'));
      }
      setFormVisible(false);
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleStartAll = async () => {
    try {
      await startAll();
      message.success(t('mcp.messages.allStarted'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleStopAll = async () => {
    try {
      await stopAll();
      message.success(t('mcp.messages.allStopped'));
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
            onClick={() => fetchServers()}
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
        onStart={startServer}
        onStop={stopServer}
        onEdit={handleEdit}
        onRemove={removeServer}
      />

      <Modal
        title={editingServer ? t('mcp.editServer') : t('mcp.addServer')}
        open={formVisible}
        onCancel={() => setFormVisible(false)}
        footer={null}
        destroyOnClose
        width={600}
      >
        <McpServerForm
          initialValues={editingServer}
          onSubmit={handleFormSubmit}
          onCancel={() => setFormVisible(false)}
          loading={loading}
        />
      </Modal>
    </div>
  );
}
