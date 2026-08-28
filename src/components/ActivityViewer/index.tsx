import { useEffect, useState } from 'react';
import { Button, Input, Popconfirm, Select, Space, Table, Tag, Typography } from 'antd';
import { DeleteOutlined, ExportOutlined, ReloadOutlined } from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import dayjs from 'dayjs';
import { useTranslation } from 'react-i18next';
import {
  useActivityStore,
  type ActivityEvent,
  type ActivityScopeFilter,
} from '@/stores/activityStore';
import { useComputerStore } from '@/stores/computerStore';

const { Search } = Input;
const { Title } = Typography;
const LEVEL_COLORS = { debug: 'default', info: 'blue', warn: 'orange', error: 'red' };
const OUTCOME_COLORS = { succeeded: 'green', failed: 'red', unknown: 'default' };
const TIME_PRESETS = [
  { label: '1h', hours: 1 },
  { label: '6h', hours: 6 },
  { label: '24h', hours: 24 },
  { label: '7d', hours: 168 },
];

interface ActivityViewerProps {
  instanceId?: string;
}

export function ActivityViewer({ instanceId }: ActivityViewerProps) {
  const { t } = useTranslation();
  const { items, total, loading, query, setQueryAndFetch, fetchActivity, exportActivity, clearActivity } =
    useActivityStore();
  const { instances, fetchInstances } = useComputerStore();
  const [searchText, setSearchText] = useState(query.keyword ?? '');

  useEffect(() => {
    setQueryAndFetch({
      start_time: undefined,
      end_time: undefined,
      levels: undefined,
      categories: undefined,
      keyword: undefined,
      limit: 50,
      offset: 0,
      scope: instanceId
        ? { kind: 'computer', computer_id: instanceId }
        : { kind: 'all' },
    });
  }, [instanceId, setQueryAndFetch]);

  useEffect(() => {
    if (!instanceId) void fetchInstances();
  }, [fetchInstances, instanceId]);

  const changeScope = (value: string) => {
    const scope: ActivityScopeFilter = value === 'all'
      ? { kind: 'all' }
      : value === 'client'
        ? { kind: 'client_only' }
        : { kind: 'computer', computer_id: value };
    void setQueryAndFetch({ scope, offset: 0 });
  };

  const scopeValue = query.scope.kind === 'all'
    ? 'all'
    : query.scope.kind === 'client_only'
      ? 'client'
      : query.scope.computer_id;

  const columns = [
    {
      title: t('logs.time'), dataIndex: 'timestamp', width: 180,
      render: (timestamp: string) => dayjs(timestamp).format('YYYY-MM-DD HH:mm:ss'),
    },
    {
      title: t('logs.level'), dataIndex: 'level', width: 80,
      render: (level: ActivityEvent['level']) => (
        <Tag color={LEVEL_COLORS[level]}>{level.toUpperCase()}</Tag>
      ),
    },
    { title: t('logs.category'), dataIndex: 'category', width: 110 },
    {
      title: t('logs.outcome'), dataIndex: 'outcome', width: 110,
      render: (outcome: ActivityEvent['outcome']) => (
        <Tag color={OUTCOME_COLORS[outcome]}>{outcome.toUpperCase()}</Tag>
      ),
    },
    {
      title: 'Scope', dataIndex: 'scope', width: 160,
      render: (scope: ActivityEvent['scope']) => scope.kind === 'client' ? 'Client' : scope.computer_id,
    },
    { title: 'Operation', dataIndex: 'operation', width: 140 },
    { title: t('logs.message'), dataIndex: 'message' },
  ];

  const handleExport = async () => {
    const path = await save({
      defaultPath: `activity-${dayjs().format('YYYY-MM-DD')}.json`,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    if (path) await exportActivity(path);
  };

  return (
    <div>
      <Title level={4}>{t('logs.title')}</Title>
      <Space wrap style={{ marginBottom: 16 }}>
        {TIME_PRESETS.map(({ label, hours }) => (
          <Button key={label} size="small" onClick={() => void setQueryAndFetch({ start_time: dayjs().subtract(hours, 'hour').toISOString(), end_time: undefined, offset: 0 })}>
            {label}
          </Button>
        ))}
        <Button size="small" onClick={() => void setQueryAndFetch({ start_time: undefined, end_time: undefined, offset: 0 })}>
          {t('logs.allTime')}
        </Button>
        <Select
          mode="multiple"
          placeholder={t('logs.filterLevel')}
          style={{ minWidth: 150 }}
          allowClear
          value={query.levels}
          onChange={(levels) => void setQueryAndFetch({ levels: levels.length ? levels : undefined, offset: 0 })}
          options={['info', 'warn', 'error', 'debug'].map((level) => ({ label: level.toUpperCase(), value: level }))}
        />
        <Select
          mode="multiple"
          placeholder={t('logs.filterCategory')}
          style={{ minWidth: 150 }}
          allowClear
          value={query.categories}
          onChange={(categories) => void setQueryAndFetch({ categories: categories.length ? categories : undefined, offset: 0 })}
          options={['system', 'mcp', 'connection', 'tool'].map((category) => ({ label: category, value: category }))}
        />
        {!instanceId && (
          <Select
            aria-label="Activity scope"
            style={{ minWidth: 200 }}
            value={scopeValue}
            onChange={changeScope}
            options={[
              { label: 'All activity', value: 'all' },
              { label: 'Client only', value: 'client' },
              ...instances.map((instance) => ({ label: instance.name, value: instance.id })),
            ]}
          />
        )}
        <Search
          placeholder={t('logs.searchKeyword')}
          allowClear
          value={searchText}
          onChange={(event) => setSearchText(event.target.value)}
          onSearch={(keyword) => void setQueryAndFetch({ keyword: keyword || undefined, offset: 0 })}
          style={{ width: 200 }}
        />
        <Button icon={<ReloadOutlined />} onClick={() => void fetchActivity()}>{t('common.refresh')}</Button>
        <Button icon={<ExportOutlined />} onClick={() => void handleExport()}>{t('logs.export')}</Button>
        <Popconfirm title={t('logs.confirmClear')} onConfirm={() => void clearActivity()}>
          <Button danger icon={<DeleteOutlined />}>{t('logs.clear')}</Button>
        </Popconfirm>
      </Space>
      <Table<ActivityEvent>
        columns={columns}
        dataSource={items}
        rowKey="id"
        loading={loading}
        size="small"
        scroll={{ x: 900 }}
        expandable={{
          expandedRowRender: (record) => <pre style={{ margin: 0, whiteSpace: 'pre-wrap' }}>{JSON.stringify(record.fields, null, 2)}</pre>,
          rowExpandable: (record) => record.fields !== undefined,
        }}
        pagination={{
          current: Math.floor((query.offset ?? 0) / (query.limit ?? 50)) + 1,
          pageSize: query.limit ?? 50,
          total,
          onChange: (page, pageSize) => void setQueryAndFetch({ limit: pageSize, offset: (page - 1) * pageSize }),
          showSizeChanger: true,
          pageSizeOptions: [20, 50, 100],
        }}
      />
    </div>
  );
}
