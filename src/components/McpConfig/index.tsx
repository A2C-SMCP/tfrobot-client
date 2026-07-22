import { useEffect, useState } from 'react';
import { Alert, App, Button, Modal, Popconfirm, Space, Table, Tag, Tooltip, Typography } from 'antd';
import {
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
  ReloadOutlined,
  ImportOutlined,
  ExportOutlined,
  SafetyCertificateOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { McpServerConfig } from '@/stores/mcpStore';
import { useSdkConfigStore, type SdkConfigServer } from '@/stores/sdkConfigStore';
import { McpServerForm } from './McpServerForm';

const { Title } = Typography;

interface McpConfigProps {
  instanceId: string;
}

export function McpConfig({ instanceId }: McpConfigProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    snapshot,
    validation,
    loading: configLoading,
    validating,
    error: configError,
    fetchConfig,
    validateConfig,
    upsertServer,
    removeServer,
    importConfig,
    exportConfig,
  } = useSdkConfigStore();

  const [formVisible, setFormVisible] = useState(false);
  const [editingServer, setEditingServer] = useState<McpServerConfig | undefined>();

  useEffect(() => {
    void fetchConfig(instanceId);
  }, [fetchConfig, instanceId]);

  const handleAdd = () => {
    setEditingServer(undefined);
    setFormVisible(true);
  };

  const handleEdit = (record: SdkConfigServer) => {
    setEditingServer(record.config);
    setFormVisible(true);
  };

  const handleFormSubmit = async (config: McpServerConfig) => {
    try {
      await upsertServer(instanceId, config);
      message.success(t(editingServer ? 'mcp.messages.updated' : 'mcp.messages.added', {
        name: config.name,
      }));
      setFormVisible(false);
    } catch (cause) {
      message.error(String(cause));
    }
  };

  const handleRemove = async (name: string) => {
    try {
      await removeServer(instanceId, name);
      message.success(t('mcp.messages.removed', { name }));
    } catch (cause) {
      message.error(String(cause));
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

  const columns = [
    {
      title: t('mcp.table.source'),
      key: 'source',
      render: (_: unknown, record: SdkConfigServer) => (
        <Space size="small">
          <Tag color={record.bundled ? 'purple' : 'blue'}>{record.origin}</Tag>
          {record.bundled && <Tag>{t('mcp.source.bundled')}</Tag>}
          {record.trustedOrigin && <Tag color="green">{t('mcp.source.trusted')}</Tag>}
        </Space>
      ),
    },
    {
      title: t('mcp.table.name'),
      dataIndex: 'name',
      key: 'name',
    },
    {
      title: t('mcp.form.bundleId'),
      dataIndex: 'bundleId',
      key: 'bundleId',
    },
    {
      title: t('mcp.table.transport'),
      key: 'transport',
      render: (_: unknown, record: SdkConfigServer) => record.config.type,
    },
    {
      title: t('mcp.table.actions'),
      key: 'actions',
      render: (_: unknown, record: SdkConfigServer) => {
        const actionHint = record.writable
          ? (record.bundled ? t('mcp.bundledConfigHint') : undefined)
          : t('mcp.readOnlyConfigHint', { origin: record.origin });
        return (
          <Space size="small">
            <Tooltip title={actionHint}>
              <span>
                <Button
                  type="text"
                  icon={<EditOutlined />}
                  title={t('mcp.actions.edit')}
                  disabled={!record.writable}
                  onClick={() => handleEdit(record)}
                />
              </span>
            </Tooltip>
            <Popconfirm
              title={t('mcp.confirmRemove')}
              onConfirm={() => handleRemove(record.name)}
              okText={t('common.yes')}
              cancelText={t('common.no')}
              disabled={!record.writable}
            >
              <Tooltip title={actionHint}>
                <span>
                  <Button
                    type="text"
                    danger
                    icon={<DeleteOutlined />}
                    title={t('mcp.actions.remove')}
                    disabled={!record.writable}
                  />
                </span>
              </Tooltip>
            </Popconfirm>
          </Space>
        );
      },
    },
  ];

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Space direction="vertical" size={0}>
          <Title level={4} style={{ margin: 0 }}>{t('mcp.configTitle')}</Title>
          {snapshot && (
            <Typography.Text type="secondary">
              {t('mcp.configRevision')}: <Typography.Text code>{snapshot.revision}</Typography.Text>
            </Typography.Text>
          )}
        </Space>
        <Space>
          <Button
            icon={<ReloadOutlined />}
            onClick={() => fetchConfig(instanceId)}
            loading={configLoading}
          >
            {t('common.refresh')}
          </Button>
          <Button
            icon={<SafetyCertificateOutlined />}
            onClick={() => validateConfig(instanceId)}
            loading={validating}
          >
            {t('mcp.validateConfig')}
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

      <Alert
        message={t('mcp.validation.schemaOnlyTitle')}
        description={t('mcp.validation.schemaOnlyDescription')}
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
      />

      {validation && (
        <Alert
          message={validation.valid
            ? t('mcp.validation.valid')
            : t('mcp.validation.invalid', { count: validation.errors.length })}
          description={!validation.valid && (
            <Space direction="vertical" size={2}>
              {validation.errors.map((item, index) => (
                <Typography.Text key={`${item.source_path ?? item.scope}:${item.field}:${index}`}>
                  {item.source_path ?? item.scope}:{item.field}: {item.reason}
                </Typography.Text>
              ))}
            </Space>
          )}
          type={validation.valid ? 'success' : 'error'}
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      {configError && (
        <Alert
          message={t('common.error')}
          description={configError}
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      <Table
        dataSource={snapshot?.mcp.servers ?? []}
        columns={columns}
        rowKey="bundleId"
        loading={configLoading}
        pagination={false}
        size="middle"
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
          loading={configLoading}
        />
      </Modal>
    </div>
  );
}
