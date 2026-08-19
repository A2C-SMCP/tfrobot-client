import { useEffect, useMemo, useState } from 'react';
import { Alert, App, Button, Modal, Popconfirm, Space, Switch, Table, Tag, Tooltip, Typography } from 'antd';
import {
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
  ReloadOutlined,
  ImportOutlined,
  ExportOutlined,
  SafetyCertificateOutlined,
  AppstoreOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  useMcpStore,
  type McpServerConfig,
  type McpServerManagedBy,
} from '@/stores/mcpStore';
import { useSdkConfigStore, type SdkConfigServer } from '@/stores/sdkConfigStore';
import type { InputDefinitionChanges } from '@/stores/inputStore';
import {
  isMissingInputDefinitionError,
  isRuntimeInputCancelledError,
} from '@/utils/runtimeActionError';
import { McpServerForm } from './McpServerForm';

const { Title } = Typography;

interface McpConfigProps {
  instanceId: string;
  onOpenPlugin?: (owner: Extract<McpServerManagedBy, { type: 'plugin' }>) => void;
}

export function McpConfig({ instanceId, onOpenPlugin }: McpConfigProps) {
  const { t } = useTranslation();
  const { message, modal } = App.useApp();
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
  const managedServers = useMcpStore((state) => (
    state.activeInstanceId === instanceId ? state.servers : []
  ));
  const managedServersReady = useMcpStore((state) => (
    state.activeInstanceId === instanceId && state.serversReady
  ));
  const managedServersLoading = useMcpStore((state) => (
    state.activeInstanceId !== instanceId || state.loading
  ));
  const managedServersError = useMcpStore((state) => (
    state.activeInstanceId === instanceId ? state.error : null
  ));
  const fetchManagedServers = useMcpStore((state) => state.fetchServers);
  const ownershipReady = managedServersReady
    && !managedServersLoading
    && managedServersError === null;

  const [formVisible, setFormVisible] = useState(false);
  const [editingServer, setEditingServer] = useState<McpServerConfig | undefined>();

  useEffect(() => {
    void fetchConfig(instanceId);
  }, [fetchConfig, instanceId]);

  useEffect(() => {
    void fetchManagedServers(instanceId);
  }, [fetchManagedServers, instanceId]);

  const pluginOwnerByBundleId = useMemo(() => {
    const result = new Map<string, Extract<McpServerManagedBy, { type: 'plugin' }>>();
    for (const server of managedServers) {
      if (server.managedBy.type === 'plugin') {
        result.set(server.bundleId, server.managedBy);
      }
    }
    return result;
  }, [managedServers]);

  const handleAdd = () => {
    if (!ownershipReady) return;
    setEditingServer(undefined);
    setFormVisible(true);
  };

  const handleEdit = (record: SdkConfigServer) => {
    if (!ownershipReady) return;
    setEditingServer(record.config);
    setFormVisible(true);
  };

  const handleFormSubmit = async (
    config: McpServerConfig,
    inputChanges?: InputDefinitionChanges,
  ) => {
    if (!ownershipReady) return;
    try {
      if (inputChanges) await upsertServer(instanceId, config, inputChanges);
      else await upsertServer(instanceId, config);
      message.success(t(editingServer ? 'mcp.messages.updated' : 'mcp.messages.added', {
        name: config.name,
      }));
      setFormVisible(false);
    } catch (cause) {
      if (isMissingInputDefinitionError(cause)) {
        message.error(t('mcp.messages.missingInputDefinition', {
          id: cause.input_id,
          name: cause.requesting_mcp?.name ?? config.name,
        }));
        return;
      }
      if (isRuntimeInputCancelledError(cause)) return;
      message.error(t('mcp.messages.operationFailed'));
    } finally {
      await fetchManagedServers(instanceId);
    }
  };

  const handleRemove = async (name: string) => {
    if (!ownershipReady) return;
    try {
      await removeServer(instanceId, name);
      message.success(t('mcp.messages.removed', { name }));
    } catch {
      message.error(t('mcp.messages.operationFailed'));
    } finally {
      await fetchManagedServers(instanceId);
    }
  };

  const handleEnabledChange = async (record: SdkConfigServer, enabled: boolean) => {
    if (!ownershipReady) return;
    const config = { ...record.config, disabled: !enabled };
    try {
      await upsertServer(instanceId, config);
      message.success(t(enabled ? 'mcp.messages.enabled' : 'mcp.messages.disabled', {
        name: record.name,
      }));
    } catch (cause) {
      if (isMissingInputDefinitionError(cause)) {
        message.error(t('mcp.messages.missingInputDefinition', {
          id: cause.input_id,
          name: cause.requesting_mcp?.name ?? config.name,
        }));
        return;
      }
      if (isRuntimeInputCancelledError(cause)) return;
      message.error(t('mcp.messages.operationFailed'));
    } finally {
      await fetchManagedServers(instanceId);
    }
  };

  const handleImport = async () => {
    if (!ownershipReady) return;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        multiple: false,
      });
      if (path) {
        const result = await (async () => {
          try {
            return await importConfig(instanceId, path as string);
          } finally {
            await fetchManagedServers(instanceId);
          }
        })();
        message.success(t('mcp.messages.importSuccess', { servers: result.servers_imported, inputs: result.inputs_imported }));
      }
    } catch {
      message.error(t('mcp.messages.operationFailed'));
    }
  };

  const exportToFile = async () => {
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
    } catch {
      message.error(t('mcp.messages.operationFailed'));
    }
  };

  const handleExport = () => {
    modal.confirm({
      title: t('mcp.exportWarning.title'),
      content: t('mcp.exportWarning.description'),
      okText: t('mcp.exportWarning.confirm'),
      okButtonProps: { danger: true },
      onOk: exportToFile,
    });
  };

  const columns = [
    {
      title: t('mcp.table.source'),
      key: 'source',
      render: (_: unknown, record: SdkConfigServer) => {
        const pluginOwner = ownershipReady
          ? pluginOwnerByBundleId.get(record.bundleId)
          : undefined;
        return (
          <Space direction="vertical" size={2}>
            <Space size="small" wrap>
              <Tag color={record.origin === 'plugin' ? 'purple' : 'blue'}>{record.origin}</Tag>
              {record.bundled && <Tag>{t('mcp.source.bundled')}</Tag>}
              {record.trustedOrigin && <Tag color="green">{t('mcp.source.trusted')}</Tag>}
            </Space>
            {pluginOwner && (
              <Typography.Text type="secondary">
                {t('mcp.pluginManagedHint', {
                  plugin: pluginOwner.plugin,
                  marketplace: pluginOwner.marketplace,
                })}
              </Typography.Text>
            )}
          </Space>
        );
      },
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
      title: t('mcp.table.enabled'),
      key: 'enabled',
      render: (_: unknown, record: SdkConfigServer) => {
        const pluginOwner = ownershipReady
          ? pluginOwnerByBundleId.get(record.bundleId)
          : undefined;
        const writable = ownershipReady && record.writable && !pluginOwner;
        const hint = writable
          ? undefined
          : ownershipReady
            ? t('mcp.readOnlyConfigHint', {
              origin: pluginOwner ? 'plugin' : record.origin,
            })
            : t('mcp.ownershipUnavailableHint');
        return (
          <Tooltip title={hint}>
            <span>
              <Switch
                checked={!record.config.disabled}
                disabled={!writable}
                onChange={(enabled) => void handleEnabledChange(record, enabled)}
                aria-label={t('mcp.actions.toggleEnabled', { name: record.name })}
              />
            </span>
          </Tooltip>
        );
      },
    },
    {
      title: t('mcp.table.actions'),
      key: 'actions',
      render: (_: unknown, record: SdkConfigServer) => {
        const pluginOwner = ownershipReady
          ? pluginOwnerByBundleId.get(record.bundleId)
          : undefined;
        if (pluginOwner) {
          return (
            <Space direction="vertical" size={2}>
              <Typography.Text type="secondary">
                {t('mcp.readOnlyConfigHint', { origin: 'plugin' })}
              </Typography.Text>
              <Button
                type="link"
                size="small"
                icon={<AppstoreOutlined />}
                aria-label={t('mcp.actions.managePlugin', { plugin: pluginOwner.plugin })}
                onClick={() => onOpenPlugin?.(pluginOwner)}
              >
                {t('mcp.actions.managePlugin', { plugin: pluginOwner.plugin })}
              </Button>
            </Space>
          );
        }
        const writable = ownershipReady && record.writable;
        const actionHint = writable
          ? (record.bundled ? t('mcp.bundledConfigHint') : undefined)
          : ownershipReady
            ? t('mcp.readOnlyConfigHint', { origin: record.origin })
            : t('mcp.ownershipUnavailableHint');
        return (
          <Space size="small">
            <Tooltip title={actionHint}>
              <span>
                <Button
                  type="text"
                  icon={<EditOutlined />}
                  title={t('mcp.actions.edit')}
                  disabled={!writable}
                  onClick={() => handleEdit(record)}
                />
              </span>
            </Tooltip>
            <Popconfirm
              title={t('mcp.confirmRemove')}
              onConfirm={() => handleRemove(record.name)}
              okText={t('common.yes')}
              cancelText={t('common.no')}
              disabled={!writable}
            >
              <Tooltip title={actionHint}>
                <span>
                  <Button
                    type="text"
                    danger
                    icon={<DeleteOutlined />}
                    title={t('mcp.actions.remove')}
                    disabled={!writable}
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
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: 12,
          marginBottom: 16,
        }}
      >
        <Space direction="vertical" size={0}>
          <Title level={4} style={{ margin: 0 }}>{t('mcp.configTitle')}</Title>
          {snapshot && (
            <Typography.Text type="secondary">
              {t('mcp.configRevision')}: <Typography.Text code>{snapshot.revision}</Typography.Text>
            </Typography.Text>
          )}
        </Space>
        <Space wrap>
          <Button
            icon={<ReloadOutlined />}
            aria-label={t('common.refresh')}
            onClick={() => {
              void Promise.all([
                fetchConfig(instanceId),
                fetchManagedServers(instanceId),
              ]);
            }}
            loading={configLoading || managedServersLoading}
          >
            {t('common.refresh')}
          </Button>
          <Button
            icon={<SafetyCertificateOutlined />}
            aria-label={t('mcp.validateConfig')}
            onClick={() => validateConfig(instanceId)}
            loading={validating}
          >
            {t('mcp.validateConfig')}
          </Button>
          <Button
            icon={<ImportOutlined />}
            aria-label={t('mcp.importConfig')}
            onClick={handleImport}
            disabled={!ownershipReady}
          >
            {t('mcp.importConfig')}
          </Button>
          <Button
            icon={<ExportOutlined />}
            aria-label={t('mcp.exportConfig')}
            onClick={handleExport}
          >
            {t('mcp.exportConfig')}
          </Button>
          <Button
            type="primary"
            icon={<PlusOutlined />}
            aria-label={t('mcp.addServer')}
            onClick={handleAdd}
            disabled={!ownershipReady}
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
          description={t('mcp.messages.operationFailed')}
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      {!ownershipReady && (
        <Alert
          message={managedServersError
            ? t('mcp.ownershipUnavailableTitle')
            : t('mcp.ownershipLoadingTitle')}
          description={t('mcp.ownershipUnavailableDescription')}
          type={managedServersError ? 'error' : 'info'}
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
        scroll={{ x: 'max-content' }}
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
          instanceId={instanceId}
          initialValues={editingServer}
          onSubmit={handleFormSubmit}
          onCancel={() => setFormVisible(false)}
          loading={configLoading}
        />
      </Modal>
    </div>
  );
}
