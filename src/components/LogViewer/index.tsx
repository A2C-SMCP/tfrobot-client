import { useEffect } from 'react';
import { Table, Button, Select, Input, Space, Tag, Typography, Popconfirm } from 'antd';
import { ExportOutlined, DeleteOutlined, ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import { useLogStore, type LogEntry } from '@/stores/logStore';

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

export function LogViewer() {
  const { t } = useTranslation();
  const { logs, loading, filter, setFilter, fetchLogs, exportLogs, clearLogs } = useLogStore();

  useEffect(() => {
    fetchLogs();
  }, []);

  const handleTimePreset = (hours: number) => {
    const start = dayjs().subtract(hours, 'hour').toISOString();
    setFilter({ start_time: start, end_time: undefined, offset: 0 });
    setTimeout(fetchLogs, 0);
  };

  const handleLevelChange = (levels: string[]) => {
    setFilter({ levels: levels.length > 0 ? levels : undefined, offset: 0 });
    setTimeout(fetchLogs, 0);
  };

  const handleCategoryChange = (categories: string[]) => {
    setFilter({ categories: categories.length > 0 ? categories : undefined, offset: 0 });
    setTimeout(fetchLogs, 0);
  };

  const handleSearch = (keyword: string) => {
    setFilter({ keyword: keyword || undefined, offset: 0 });
    setTimeout(fetchLogs, 0);
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
    setFilter({ limit: pageSize, offset: (page - 1) * pageSize });
    setTimeout(fetchLogs, 0);
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
        <Button size="small" onClick={() => { setFilter({ start_time: undefined, end_time: undefined, offset: 0 }); setTimeout(fetchLogs, 0); }}>
          {t('logs.allTime')}
        </Button>

        <Select
          mode="multiple"
          placeholder={t('logs.filterLevel')}
          style={{ minWidth: 150 }}
          allowClear
          onChange={handleLevelChange}
          options={['info', 'warn', 'error', 'debug'].map((l) => ({ label: l.toUpperCase(), value: l }))}
        />

        <Select
          mode="multiple"
          placeholder={t('logs.filterCategory')}
          style={{ minWidth: 150 }}
          allowClear
          onChange={handleCategoryChange}
          options={['system', 'mcp', 'connection', 'tool'].map((c) => ({ label: c, value: c }))}
        />

        <Search
          placeholder={t('logs.searchKeyword')}
          allowClear
          onSearch={handleSearch}
          style={{ width: 200 }}
        />

        <Button icon={<ReloadOutlined />} onClick={fetchLogs}>{t('common.refresh')}</Button>
        <Button icon={<ExportOutlined />} onClick={handleExport}>{t('logs.export')}</Button>
        <Popconfirm title={t('logs.confirmClear')} onConfirm={() => clearLogs()}>
          <Button danger icon={<DeleteOutlined />}>{t('logs.clear')}</Button>
        </Popconfirm>
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
