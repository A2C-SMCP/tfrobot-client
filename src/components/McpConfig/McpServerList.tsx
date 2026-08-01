import { Table, Button, Space, Tag, Tooltip, Typography } from 'antd';
import {
  AppstoreOutlined,
  PlayCircleOutlined,
  PauseCircleOutlined,
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
  onStop?: (bundleId: string, name: string) => Promise<void>;
  onOpenPlugin?: (owner: PluginMcpServerOwner) => void;
}

export function McpServerList({
  servers,
  actionsDisabled = false,
  actionsDisabledReason,
  loading,
  onStart,
  onStop,
  onOpenPlugin,
}: McpServerListProps) {
  const { t } = useTranslation();
  const safeStatusLabel = (record: McpServerStatus) => {
    if (record.running) return t('mcp.status.running');
    if (record.status_message === 'error') return t('mcp.status.error');
    if (record.status_message === 'pending') return t('mcp.status.pending');
    return t('mcp.status.stopped');
  };

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
      dataIndex: 'name',
      key: 'name',
    },
    {
      title: t('mcp.table.status'),
      key: 'status',
      render: (_: unknown, record: McpServerStatus) => (
        <ServerStatusBadge
          running={record.running}
          statusMessage={record.status_message}
        />
      ),
    },
    {
      title: t('mcp.table.message'),
      key: 'status_message',
      ellipsis: true,
      render: (_: unknown, record: McpServerStatus) => safeStatusLabel(record),
    },
    {
      title: t('mcp.table.actions'),
      key: 'actions',
      render: (_: unknown, record: McpServerStatus) => {
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
        return (
          <Space size="small">
            {record.running ? (
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
