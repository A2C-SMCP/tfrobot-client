import { Table, Button, Space, Tag, Tooltip, Typography } from 'antd';
import {
  AppstoreOutlined,
  PlayCircleOutlined,
  PauseCircleOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { ServerStatusBadge } from './ServerStatusBadge';
import type { McpServerManagedBy, McpServerStatus } from '@/stores/mcpStore';

type PluginMcpServerOwner = Extract<McpServerManagedBy, { type: 'plugin' }>;

interface McpServerListProps {
  servers: McpServerStatus[];
  actionsDisabled?: boolean;
  actionsDisabledReason?: string;
  loading?: boolean;
  onStart?: (bundleId: string) => Promise<void>;
  onRetry?: (bundleId: string, name: string) => Promise<void>;
  onStop?: (bundleId: string, name: string) => Promise<void>;
  onOpenPlugin?: (owner: PluginMcpServerOwner) => void;
  onAuthorize?: (bundleId: string) => Promise<void>;
  onCancelAuthorization?: (bundleId: string) => Promise<void>;
  onClearAuthorization?: (bundleId: string) => Promise<void>;
}

export function McpServerList({
  servers,
  actionsDisabled = false,
  actionsDisabledReason,
  loading,
  onStart,
  onRetry,
  onStop,
  onOpenPlugin,
  onAuthorize,
  onCancelAuthorization,
  onClearAuthorization,
}: McpServerListProps) {
  const { t } = useTranslation();

  const handleStart = async (bundleId: string) => {
    await onStart?.(bundleId);
  };

  const pluginLifecycleMessage = (record: McpServerStatus) => {
    if (record.managedBy.type !== 'plugin') {
      return undefined;
    }
    return t('mcp.pluginManagedHint', {
      plugin: record.managedBy.plugin,
      marketplace: record.managedBy.marketplace,
    });
  };

  const columns = [
    {
      title: t('mcp.table.source'),
      key: 'source',
      render: (_: unknown, record: McpServerStatus) => {
        if (record.managedBy.type === 'built_in') {
          return <Tag color="blue">{t('mcp.source.robotControl')}</Tag>;
        }
        if (record.managedBy.type === 'plugin') {
          return (
            <Space direction="vertical" size={2}>
              <Tag color="purple">
                {t('mcp.source.plugin', { plugin: record.managedBy.plugin })}
              </Tag>
              <Typography.Text type="secondary">
                {t('mcp.source.marketplace', { marketplace: record.managedBy.marketplace })}
              </Typography.Text>
            </Space>
          );
        }
        return <Tag>{t('mcp.source.user')}</Tag>;
      },
    },
    {
      title: t('mcp.table.name'),
      key: 'name',
      render: (_: unknown, record: McpServerStatus) => (
        record.managedBy.type === 'built_in'
          ? t('mcp.builtIn.robotControlName')
          : record.name
      ),
    },
    {
      title: t('mcp.table.status'),
      key: 'status',
      render: (_: unknown, record: McpServerStatus) => (
        <ServerStatusBadge server={record} />
      ),
    },
    {
      title: t('mcp.table.authorization'),
      key: 'authorization',
      render: (_: unknown, record: McpServerStatus) => {
        // Older cached/test snapshots predate the authorization field; treat them as non-OAuth.
        const oauthStatus = record.oauth_status ?? { state: 'not_applicable' as const };
        const interaction = record.oauth_interaction
          ?? (oauthStatus.state === 'not_applicable' ? 'none' : 'interactive');
        if (interaction === 'machine') {
          const label = (() => {
            switch (oauthStatus.state) {
              case 'authorized': return t('mcp.authorization.authorized');
              case 'authorization_pending': return t('mcp.authorization.pending');
              case 'reauthorization_required':
                return t('mcp.authorization.requiredScope', { scope: oauthStatus.required_scope });
              case 'error': return t('mcp.authorization.error');
              default: return t('mcp.authorization.unauthorized');
            }
          })();
          return <Tag color={oauthStatus.state === 'authorized' ? 'green' : undefined}>{label}</Tag>;
        }
        switch (oauthStatus.state) {
          case 'not_applicable':
            return <Typography.Text type="secondary">—</Typography.Text>;
          case 'authorization_pending':
            return (
              <Button
                size="small"
                onClick={() => onCancelAuthorization?.(record.bundleId)}
                disabled={actionsDisabled}
              >
                {t('mcp.authorization.cancel')}
              </Button>
            );
          case 'authorized':
            return (
              <Space direction="vertical" size={2}>
                <Tag color="green">{t('mcp.authorization.authorized')}</Tag>
                <Button
                  type="link"
                  size="small"
                  onClick={() => onClearAuthorization?.(record.bundleId)}
                  disabled={actionsDisabled}
                >
                  {t('mcp.authorization.clear')}
                </Button>
              </Space>
            );
          case 'reauthorization_required':
            return (
              <Space direction="vertical" size={2}>
                <Typography.Text type="secondary">
                  {t('mcp.authorization.requiredScope', { scope: oauthStatus.required_scope })}
                </Typography.Text>
                <Space size="small">
                  <Button
                    type="primary"
                    size="small"
                    onClick={() => onAuthorize?.(record.bundleId)}
                    disabled={actionsDisabled}
                  >
                    {t('mcp.authorization.reauthorize')}
                  </Button>
                  <Button
                    type="link"
                    size="small"
                    onClick={() => onClearAuthorization?.(record.bundleId)}
                    disabled={actionsDisabled}
                  >
                    {t('mcp.authorization.clear')}
                  </Button>
                </Space>
              </Space>
            );
          case 'error':
            return (
              <Space direction="vertical" size={2}>
                <Typography.Text type="danger">
                  {t('mcp.authorization.error')}
                </Typography.Text>
                <Button
                  type="primary"
                  size="small"
                  onClick={() => onAuthorize?.(record.bundleId)}
                  disabled={actionsDisabled}
                >
                  {t('mcp.authorization.authorize')}
                </Button>
              </Space>
            );
          default:
            return (
              <Button
                type="primary"
                size="small"
                onClick={() => onAuthorize?.(record.bundleId)}
                disabled={actionsDisabled}
              >
                {t('mcp.authorization.authorize')}
              </Button>
            );
        }
      },
    },
    {
      title: t('mcp.table.actions'),
      key: 'actions',
      render: (_: unknown, record: McpServerStatus) => {
        if (record.managedBy.type === 'built_in') {
          return (
            <Typography.Text type="secondary">
              {t('mcp.builtIn.robotControlManagedHint')}
            </Typography.Text>
          );
        }
        if (record.managedBy.type === 'plugin') {
          return (
            <Space direction="vertical" size={2}>
              <Typography.Text type="secondary">
                {pluginLifecycleMessage(record)}
              </Typography.Text>
              <Button
                type="link"
                size="small"
                icon={<AppstoreOutlined />}
                onClick={() => onOpenPlugin?.(record.managedBy as PluginMcpServerOwner)}
              >
                {t('mcp.actions.managePlugin', { plugin: record.managedBy.plugin })}
              </Button>
            </Space>
          );
        }
        const retryable = record.activation_state === 'started'
          && (record.connection_state === 'disconnected' || record.connection_state === 'error');
        return (
          <Space size="small">
            {retryable && (
              <Tooltip title={actionsDisabled ? actionsDisabledReason : t('mcp.actions.retry')}>
                <span>
                  <Button
                    type="text"
                    icon={<ReloadOutlined />}
                    onClick={() => onRetry?.(record.bundleId, record.name)}
                    title={t('mcp.actions.retry')}
                    disabled={actionsDisabled}
                  />
                </span>
              </Tooltip>
            )}
            {record.activation_state === 'started' ? (
              <Tooltip title={actionsDisabled ? actionsDisabledReason : undefined}>
                <span>
                  <Button
                    type="text"
                    icon={<PauseCircleOutlined />}
                    onClick={() => onStop?.(record.bundleId, record.name)}
                    title={t('mcp.actions.stop')}
                    disabled={actionsDisabled}
                  />
                </span>
              </Tooltip>
            ) : (
              <Tooltip title={actionsDisabled ? actionsDisabledReason : undefined}>
                <span>
                  <Button
                    type="text"
                    icon={<PlayCircleOutlined />}
                    onClick={() => handleStart(record.bundleId)}
                    title={t('mcp.actions.start')}
                    disabled={actionsDisabled}
                  />
                </span>
              </Tooltip>
            )}
          </Space>
        );
      },
    },
  ];

  return (
    <Table
      dataSource={servers}
      columns={columns}
      rowKey="bundleId"
      loading={loading}
      pagination={false}
      size="middle"
      scroll={{ x: 720 }}
    />
  );
}
