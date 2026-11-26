import { usePageAction } from '@/components/Navigation/pageActivityState';
import { useStore } from 'zustand';
import { usePageActive } from '@/components/Navigation/pageActivityState';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { PagePopconfirm as Popconfirm } from '@/components/Navigation/PageOverlays';
import { useEffect, useMemo, useRef, useState } from 'react';
import { Alert, Button, Input, Select, Space, Spin, Table, Tag, Typography } from 'antd';
import { DeleteOutlined, ExportOutlined, ReloadOutlined } from '@ant-design/icons';
import { save } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import dayjs from 'dayjs';
import { useTranslation } from 'react-i18next';
import {
  activityViewStore,
  type ActivityEvent,
  type ActivityScopeFilter,
} from '@/stores/activityStore';
import { useComputerStore } from '@/stores/computerStore';
import type { ToolCallHistoryRecord } from '@/stores/debugStore';
import { ACTIVITY_CATEGORIES } from './categories';

const { Search } = Input;
const { Title } = Typography;
const LEVEL_COLORS = { debug: 'default', info: 'blue', warn: 'orange', error: 'red' };
const OUTCOME_COLORS = { succeeded: 'green', failed: 'red', unknown: 'default' };
const TIME_PRESETS = [
  { hours: 1 },
  { hours: 6 },
  { hours: 24 },
  { hours: 168, days: 7 },
];
interface ActivityViewerProps {
  instanceId?: string;
  focusRequest?: { correlationId?: string };
}

function fieldsOf(record: ActivityEvent) {
  return record.fields && typeof record.fields === 'object' ? record.fields as Record<string, unknown> : {};
}

function ToolActivityDetails({ instanceId, requestId, record }: { instanceId: string; requestId: string; record: ActivityEvent }) {
  const { t } = useTranslation();
  const active = usePageActive();
  const [details, setDetails] = useState<ToolCallHistoryRecord | null>();
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (!active) return;
    let current = true;
    setFailed(false);
    setDetails(undefined);
    void invoke<ToolCallHistoryRecord[]>('get_tool_history', { instanceId, requestId }).then((records) => {
      if (current) setDetails(records?.find(row => row.req_id === requestId && row.computer_instance_id === instanceId) ?? null);
    }).catch(() => { if (current) setFailed(true); });
    return () => { current = false; };
  }, [active, instanceId, requestId]);

  const activityError = fieldsOf(record).error;
  const reason = details?.error ?? (typeof activityError === 'string' ? activityError : undefined);
  return <Space direction="vertical" style={{ width: '100%' }}>
    {reason && <Alert type="error" message={t('logs.failureReason')} description={reason} />}
    {failed ? <Alert type="warning" message={t('logs.toolDetailsUnavailable')} /> : details === undefined ? <Spin /> : details && <>
      <Typography.Text strong>{t('debug.historyColumns.params')}</Typography.Text>
      <pre style={{ margin: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>{JSON.stringify(details.parameters, null, 2)}</pre>
    </>}
  </Space>;
}

export function ActivityViewer({ instanceId, focusRequest }: ActivityViewerProps) {
  const action = usePageAction();
  const { t } = useTranslation();
  const store = useMemo(() => activityViewStore(instanceId), [instanceId]);
  const active = usePageActive();
  const { items, total, loading, error, query, invalidated, setQueryAndFetch, focusToolCall, clearToolFocus,
    fetchActivity, exportActivity, clearActivity } = useStore(store);
  const { instances, fetchInstances } = useComputerStore();
  const [searchText, setSearchText] = useNavigationState('activity.search', query.keyword ?? '');
  const handledFocus = useRef<ActivityViewerProps['focusRequest']>(undefined);
  const expandedFocus = useRef<ActivityViewerProps['focusRequest']>(undefined);
  const tableRef = useRef<HTMLDivElement>(null);
  const categoryOptions = [...new Set([
    ...ACTIVITY_CATEGORIES,
    ...items.map((item) => item.category),
  ])];
  const categoryLabel = (category: string) => t(`logs.categories.${category}`, { defaultValue: category });

  useEffect(() => {
    if (!active) return;
    if (focusRequest && focusRequest !== handledFocus.current) {
      handledFocus.current = focusRequest;
      if (focusRequest.correlationId) {
        setSearchText('');
        void focusToolCall(focusRequest.correlationId);
        return;
      }
      if (store.getState().query.correlation_id) {
        void clearToolFocus();
        setSearchText(store.getState().query.keyword ?? '');
        return;
      }
    }
    void fetchActivity();
  }, [active, fetchActivity, focusRequest, focusToolCall, clearToolFocus, setSearchText, store]);

  useEffect(() => {
    if (active && invalidated && !loading) void fetchActivity();
  }, [active, invalidated, loading, fetchActivity]);

  useEffect(() => {
    if (active && !instanceId) void fetchInstances();
  }, [active, fetchInstances, instanceId]);

  const [expandedRows, setExpandedRows] = useNavigationState<React.Key[]>('activity.expanded', []);
  useEffect(() => {
    if (!active || loading || !focusRequest?.correlationId || expandedFocus.current === focusRequest) return;
    if (query.correlation_id !== focusRequest.correlationId) return;
    const matching = items.find((record) => record.correlation_id === focusRequest.correlationId && record.event_type === 'tool_call');
    if (matching) {
      expandedFocus.current = focusRequest;
      setExpandedRows((rows) => rows.includes(matching.id) ? rows : [...rows, matching.id]);
      const row = tableRef.current?.querySelector<HTMLElement>(`[data-activity-id="${matching.id}"]`);
      row?.scrollIntoView?.({ block: 'nearest' });
      row?.focus({ preventScroll: true });
    }
  }, [active, loading, focusRequest, query.correlation_id, items, setExpandedRows]);

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
    { title: t('logs.category'), dataIndex: 'category', width: 110, render: categoryLabel },
    {
      title: t('logs.outcome'), dataIndex: 'outcome', width: 110,
      render: (outcome: ActivityEvent['outcome']) => (
        <Tag color={OUTCOME_COLORS[outcome]}>{t(`logs.outcomes.${outcome}`)}</Tag>
      ),
    },
    instanceId ? {
      title: t('logs.source'), key: 'source', width: 140,
      render: (_: unknown, record: ActivityEvent) => {
        const { server_name: server, bundle_id: bundle } = fieldsOf(record);
        return typeof server === 'string' ? server : typeof bundle === 'string' ? bundle : t('logs.client');
      },
    } : {
      title: t('logs.scope'), dataIndex: 'scope', width: 160,
      render: (scope: ActivityEvent['scope']) => scope.kind === 'client' ? t('logs.client') : scope.computer_id,
    },
    { title: t('logs.operation'), dataIndex: 'operation', width: 140 },
    { title: t('logs.message'), dataIndex: 'message' },
  ];

  const handleExport = async () => {
    const current = action();
    const path = await save({
      defaultPath: `activity-${dayjs().format('YYYY-MM-DD')}.json`,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    if (path && current()) await exportActivity(path);
  };

  return (
    <div>
      <Title level={4}>{t('logs.title')}</Title>
      <Space wrap style={{ marginBottom: 16 }}>
        {TIME_PRESETS.map(({ hours, days }) => (
          <Button key={hours} size="small" onClick={() => void setQueryAndFetch({ start_time: dayjs().subtract(hours, 'hour').toISOString(), end_time: undefined, offset: 0 })}>
            {days ? t('logs.pastDays', { count: days }) : t('logs.pastHours', { count: hours })}
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
          aria-label={t('logs.categoryLabel')}
          placeholder={t('logs.filterCategory')}
          style={{ minWidth: 150 }}
          allowClear
          value={query.categories}
          onChange={(categories) => void setQueryAndFetch({ categories: categories.length ? categories : undefined, offset: 0 })}
          options={categoryOptions.map((category) => ({ label: categoryLabel(category), value: category }))}
        />
        {instanceId && <Select<ActivityEvent['outcome'] | 'all'>
          aria-label={t('logs.outcome')}
          value={query.outcome ?? 'all'} onChange={(value) => void setQueryAndFetch({ outcome: value === 'all' ? undefined : value, offset: 0 })}
          options={[
            { value: 'all' as const, label: t('logs.allOutcomes') },
            ...(['succeeded', 'failed', 'unknown'] as const).map(outcome => ({ value: outcome, label: t(`logs.outcomes.${outcome}`) })),
          ]}
        />}
        {!instanceId && (
          <Select
            aria-label={t('logs.filterScope')}
            style={{ minWidth: 200 }}
            value={scopeValue}
            onChange={changeScope}
            options={[
              { label: t('logs.allScopes'), value: 'all' },
              { label: t('logs.clientOnly'), value: 'client' },
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
        {query.correlation_id && <Button type="link" onClick={() => {
          void clearToolFocus();
          setSearchText(store.getState().query.keyword ?? '');
        }}>
          {t('logs.clearCallFocus')}
        </Button>}
        <Button icon={<ExportOutlined />} onClick={() => void handleExport()}>{t('logs.export')}</Button>
        <Popconfirm title={t('logs.confirmClear')} onConfirm={() => void clearActivity()}>
          <Button danger icon={<DeleteOutlined />}>{t('logs.clear')}</Button>
        </Popconfirm>
      </Space>
      {error && <Alert type="error" showIcon message={error} style={{ marginBottom: 16 }} />}
      <div ref={tableRef}>
      <Table<ActivityEvent>
        columns={columns}
        dataSource={items}
        rowKey="id"
        onRow={(record) => ({ 'data-activity-id': record.id, tabIndex: -1 })}
        loading={loading}
        size="small"
        scroll={{ x: 900 }}
        expandable={{
          expandedRowKeys: expandedRows,
          onExpandedRowsChange: (keys) => setExpandedRows([...keys]),
          expandedRowRender: (record) => (
            <Space direction="vertical" style={{ width: '100%' }}>
              {instanceId && record.event_type === 'tool_call' && record.correlation_id && expandedRows.includes(record.id) && (
                <ToolActivityDetails key={record.correlation_id} instanceId={instanceId} requestId={record.correlation_id} record={record} />
              )}
              <pre style={{ margin: 0, whiteSpace: 'pre-wrap' }}>
              {JSON.stringify({
                event_type: record.event_type,
                correlation_id: record.correlation_id,
                fields: record.fields,
              }, null, 2)}
              </pre>
            </Space>
          ),
          rowExpandable: (record) => record.fields !== undefined || record.correlation_id !== undefined,
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
    </div>
  );
}
