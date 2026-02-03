import { Table, Button, Space, Popconfirm, message } from 'antd';
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
  loading?: boolean;
  onStart: (name: string) => Promise<void>;
  onStop: (name: string) => Promise<void>;
  onEdit: (name: string) => void;
  onRemove: (name: string) => Promise<void>;
}

export function McpServerList({
  servers,
  loading,
  onStart,
  onStop,
  onEdit,
  onRemove,
}: McpServerListProps) {
  const { t } = useTranslation();

  const handleStart = async (name: string) => {
    try {
      await onStart(name);
      message.success(t('mcp.messages.started', { name }));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleStop = async (name: string) => {
    try {
      await onStop(name);
      message.success(t('mcp.messages.stopped', { name }));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleRemove = async (name: string) => {
    try {
      await onRemove(name);
      message.success(t('mcp.messages.removed', { name }));
    } catch (e) {
      message.error(String(e));
    }
  };

  const columns = [
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
          {record.running ? (
            <Button
              type="text"
              icon={<PauseCircleOutlined />}
              onClick={() => handleStop(record.name)}
              title={t('mcp.actions.stop')}
            />
          ) : (
            <Button
              type="text"
              icon={<PlayCircleOutlined />}
              onClick={() => handleStart(record.name)}
              title={t('mcp.actions.start')}
            />
          )}
          <Button
            type="text"
            icon={<EditOutlined />}
            onClick={() => onEdit(record.name)}
            title={t('mcp.actions.edit')}
          />
          <Popconfirm
            title={t('mcp.confirmRemove')}
            onConfirm={() => handleRemove(record.name)}
            okText={t('common.yes')}
            cancelText={t('common.no')}
          >
            <Button
              type="text"
              danger
              icon={<DeleteOutlined />}
              title={t('mcp.actions.remove')}
            />
          </Popconfirm>
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
