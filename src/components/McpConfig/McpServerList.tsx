import { App, Table, Button, Space, Popconfirm, Tag, Tooltip } from 'antd';
import {
  PlayCircleOutlined,
  PauseCircleOutlined,
  EditOutlined,
  DeleteOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { ServerStatusBadge } from './ServerStatusBadge';
import type { McpServerStatus } from '@/stores/mcpStore';

interface McpServerListProps {
  servers: McpServerStatus[];
  mode?: 'config' | 'runtime';
  actionsDisabled?: boolean;
  loading?: boolean;
  onStart?: (name: string) => Promise<void>;
  onStop?: (name: string) => Promise<void>;
  onEdit?: (name: string) => void;
  onRemove?: (name: string) => Promise<void>;
}

export function McpServerList({
  servers,
  mode = 'config',
  actionsDisabled = false,
  loading,
  onStart,
  onStop,
  onEdit,
  onRemove,
}: McpServerListProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();

  const handleStart = async (name: string) => {
    await onStart?.(name);
  };

  const handleStop = async (name: string) => {
    try {
      await onStop?.(name);
      message.success(t('mcp.messages.stopped', { name }));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleRemove = async (name: string) => {
    try {
      await onRemove?.(name);
      message.success(t('mcp.messages.removed', { name }));
    } catch (e) {
      message.error(String(e));
    }
  };

  const isPluginOwned = (record: McpServerStatus) => record.managedBy.type === 'plugin';

  const pluginLifecycleMessage = (record: McpServerStatus) => {
    if (record.managedBy.type !== 'plugin') {
      return undefined;
    }
    return t('mcp.pluginManagedHint', {
      plugin: record.managedBy.plugin,
      marketplace: record.managedBy.marketplace,
    });
  };

  const sourceLabel = (record: McpServerStatus) => {
    if (record.managedBy.type === 'plugin') {
      return `${record.managedBy.plugin}@${record.managedBy.marketplace}`;
    }
    return t('mcp.source.user');
  };

  const columns = [
    {
      title: t('mcp.table.source'),
      key: 'source',
      render: (_: unknown, record: McpServerStatus) => (
        <Tag color={isPluginOwned(record) ? 'purple' : 'default'}>
          {sourceLabel(record)}
        </Tag>
      ),
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
      dataIndex: 'status_message',
      key: 'status_message',
      ellipsis: true,
    },
    {
      title: t('mcp.table.actions'),
      key: 'actions',
      render: (_: unknown, record: McpServerStatus) => (
        <Space size="small">
          {mode === 'runtime' && (record.running ? (
            <Tooltip title={pluginLifecycleMessage(record)}>
              <span>
                <Button
                  type="text"
                  icon={<PauseCircleOutlined />}
                  onClick={() => handleStop(record.name)}
                  title={t('mcp.actions.stop')}
                  disabled={actionsDisabled || isPluginOwned(record)}
                />
              </span>
            </Tooltip>
          ) : (
            <Tooltip title={pluginLifecycleMessage(record)}>
              <span>
                <Button
                  type="text"
                  icon={<PlayCircleOutlined />}
                  onClick={() => handleStart(record.name)}
                  title={t('mcp.actions.start')}
                  disabled={actionsDisabled || isPluginOwned(record)}
                />
              </span>
            </Tooltip>
          ))}
          {mode === 'config' && <Tooltip title={pluginLifecycleMessage(record)}>
            <span>
              <Button
                type="text"
                icon={<EditOutlined />}
                onClick={() => onEdit?.(record.name)}
                title={t('mcp.actions.edit')}
                disabled={isPluginOwned(record)}
              />
            </span>
          </Tooltip>}
          {mode === 'config' && <Popconfirm
            title={t('mcp.confirmRemove')}
            onConfirm={() => handleRemove(record.name)}
            okText={t('common.yes')}
            cancelText={t('common.no')}
            disabled={isPluginOwned(record)}
          >
            <Tooltip title={pluginLifecycleMessage(record)}>
              <span>
                <Button
                  type="text"
                  danger
                  icon={<DeleteOutlined />}
                  title={t('mcp.actions.remove')}
                  disabled={isPluginOwned(record)}
                />
              </span>
            </Tooltip>
          </Popconfirm>}
        </Space>
      ),
    },
  ];

  return (
    <Table
      dataSource={servers}
      columns={columns}
      rowKey="name"
      loading={loading}
      pagination={false}
      size="middle"
    />
  );
}
