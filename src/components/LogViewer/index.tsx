import { useEffect, useState } from 'react';
import { Table, Button, Select, Input, Space, Tag, Typography, Popconfirm } from 'antd';
import { ExportOutlined, DeleteOutlined, ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import { useLogStore, type LogEntry } from '@/stores/logStore';
import { useComputerStore } from '@/stores/computerStore';

const { Title } = Typography;
const { Search } = Input;

const LEVEL_COLORS: Record<string, string> = {
  info: 'blue',
  warn: 'orange',
  error: 'red',
  debug: 'default',
};

const TIME_PRESETS = [
  { label: '1h', hours: 1 },
  { label: '6h', hours: 6 },
  { label: '24h', hours: 24 },
  { label: '7d', hours: 168 },
];

const DEFAULT_LOG_FILTER = {
  start_time: undefined,
  end_time: undefined,
  levels: undefined,
  categories: undefined,
  keyword: undefined,
  limit: 50,
  offset: 0,
};

interface LogViewerProps {
  instanceId?: string;
}

export function LogViewer({ instanceId }: LogViewerProps) {
  const { t } = useTranslation();
  const { logs, loading, filter, setFilterAndFetch, fetchLogs, exportLogs, clearLogs } = useLogStore();
  const { instances, fetchInstances } = useComputerStore();
  const [searchText, setSearchText] = useState(filter.keyword ?? '');

  useEffect(() => {
    setFilterAndFetch({
      ...DEFAULT_LOG_FILTER,
      computer_instance_id: instanceId,
    });
  }, [instanceId, setFilterAndFetch]);

  useEffect(() => {
    if (!instanceId) {
      fetchInstances();
    }
  }, [fetchInstances, instanceId]);

  useEffect(() => {
    setSearchText(filter.keyword ?? '');
  }, [filter.keyword]);

  const handleTimePreset = (hours: number) => {
    const start = dayjs().subtract(hours, 'hour').toISOString();
    setFilterAndFetch({ start_time: start, end_time: undefined, offset: 0 });
  };

  const handleLevelChange = (levels: string[]) => {
    setFilterAndFetch({ levels: levels.length > 0 ? levels : undefined, offset: 0 });
  };

  const handleCategoryChange = (categories: string[]) => {
    setFilterAndFetch({ categories: categories.length > 0 ? categories : undefined, offset: 0 });
  };

  const handleSearch = (keyword: string) => {
    setSearchText(keyword);
    setFilterAndFetch({ keyword: keyword || undefined, offset: 0 });
  };

  const handleComputerChange = (computerInstanceId?: string) => {
    setFilterAndFetch({ computer_instance_id: computerInstanceId, offset: 0 });
  };

  const handleExport = async () => {
    const path = await save({
      defaultPath: `logs-${dayjs().format('YYYY-MM-DD')}.json`,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    if (path) {
      await exportLogs(path);
    }
  };

  const handlePageChange = (page: number, pageSize: number) => {
    setFilterAndFetch({ limit: pageSize, offset: (page - 1) * pageSize });
  };

  const columns = [
    {
      title: t('logs.time'),
      dataIndex: 'timestamp',
      key: 'timestamp',
      width: 180,
      render: (ts: string) => dayjs(ts).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: t('logs.level'),
      dataIndex: 'level',
      key: 'level',
      width: 80,
      render: (level: string) => (
        <Tag color={LEVEL_COLORS[level] || 'default'}>{level.toUpperCase()}</Tag>
      ),
    },
    {
      title: t('logs.category'),
      dataIndex: 'category',
      key: 'category',
      width: 100,
    },
    {
      title: 'Computer',
      dataIndex: 'computer_instance_id',
      key: 'computer_instance_id',
      width: 140,
      render: (value?: string) => value || '-',
    },
    {
      title: t('logs.message'),
      dataIndex: 'message',
      key: 'message',
    },
  ];

  return (
    <div>
      <Title level={4}>{t('logs.title')}</Title>

      <Space wrap style={{ marginBottom: 16 }}>
        {TIME_PRESETS.map((p) => (
          <Button key={p.label} size="small" onClick={() => handleTimePreset(p.hours)}>
            {p.label}
          </Button>
        ))}
        <Button size="small" onClick={() => setFilterAndFetch({ start_time: undefined, end_time: undefined, offset: 0 })}>
          {t('logs.allTime')}
        </Button>

        <Select
          mode="multiple"
          placeholder={t('logs.filterLevel')}
          style={{ minWidth: 150 }}
          allowClear
          value={filter.levels}
          onChange={handleLevelChange}
          options={['info', 'warn', 'error', 'debug'].map((l) => ({ label: l.toUpperCase(), value: l }))}
        />

        <Select
          mode="multiple"
          placeholder={t('logs.filterCategory')}
          style={{ minWidth: 150 }}
          allowClear
          value={filter.categories}
          onChange={handleCategoryChange}
          options={['system', 'mcp', 'connection', 'tool'].map((c) => ({ label: c, value: c }))}
        />

        {!instanceId && (
          <Select
            placeholder="Computer"
            style={{ minWidth: 180 }}
            allowClear
            value={filter.computer_instance_id}
            onChange={handleComputerChange}
            options={instances.map((instance) => ({ label: instance.name, value: instance.id }))}
          />
        )}

        <Search
          placeholder={t('logs.searchKeyword')}
          allowClear
          value={searchText}
          onChange={(event) => setSearchText(event.target.value)}
          onSearch={handleSearch}
          style={{ width: 200 }}
        />

        <Button icon={<ReloadOutlined />} onClick={fetchLogs}>{t('common.refresh')}</Button>
        <Button icon={<ExportOutlined />} onClick={handleExport}>{t('logs.export')}</Button>
        {!instanceId && (
          <Popconfirm title={t('logs.confirmClear')} onConfirm={() => clearLogs()}>
            <Button danger icon={<DeleteOutlined />}>{t('logs.clear')}</Button>
          </Popconfirm>
        )}
      </Space>

      <Table<LogEntry>
        columns={columns}
        dataSource={logs}
        rowKey="id"
        loading={loading}
        size="small"
        expandable={{
          expandedRowRender: (record) =>
            record.details ? <pre style={{ margin: 0, whiteSpace: 'pre-wrap' }}>{record.details}</pre> : null,
          rowExpandable: (record) => !!record.details,
        }}
        pagination={{
          pageSize: filter.limit || 50,
          onChange: handlePageChange,
          showSizeChanger: true,
          pageSizeOptions: [20, 50, 100],
        }}
      />
    </div>
  );
}
