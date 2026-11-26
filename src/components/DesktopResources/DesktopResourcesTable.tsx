import { PageModal } from '@/components/Navigation/PageOverlays';
import { usePageActive } from '@/components/Navigation/pageActivityState';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { useEffect, useState } from 'react';
import { Alert, Button, Empty, Input, Space, Spin, Table, Typography } from 'antd';
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { McpSourceFilter } from '@/components/McpConfig/McpSourceFilter';
import { NoticeBar } from '@/components/common/NoticeBar';
import { useNoticeLifecycle } from '@/components/common/useNoticeLifecycle';
import { desktopWindowKey, type DesktopInstanceState, type DesktopWindow } from '@/stores/desktopStore';
import { WindowContentPreview } from './WindowContentPreview';

const { Text } = Typography;

interface DesktopResourcesTableProps {
  desktop: DesktopInstanceState;
  instanceId: string;
  runtimeKey: string;
  fetchWindowDetail: (instanceId: string, runtimeKey: string, bundleId: string, uri: string) => Promise<void>;
}

export function DesktopResourcesTable({ desktop, instanceId, runtimeKey, fetchWindowDetail }: DesktopResourcesTableProps) {
  const { t } = useTranslation();
  const active = usePageActive();
  const enumerationNotice = useNoticeLifecycle(
    'desktop-enumeration-unverified',
    desktop.enumerationStatus === 'unverified' && desktop.windows.length > 0,
  );
  const [search, setSearch] = useNavigationState('desktop.search', '');
  const [sources, setSources] = useNavigationState<string[] | undefined>('desktop.sources', undefined);
  const [selection, setSelection] = useState<{ instanceId: string; runtimeKey: string; key: string } | null>(null);
  const selected = selection?.instanceId === instanceId && selection.runtimeKey === runtimeKey
    ? desktop.windows.find((window) => desktopWindowKey(window) === selection.key)
    : undefined;

  useEffect(() => {
    if (!active || !selected) setSelection(null);
  }, [active, selected]);

  const openWindow = (record: DesktopWindow) => {
    const key = desktopWindowKey(record);
    setSelection({ instanceId, runtimeKey, key });
    if (!desktop.windowDetails[key] && !desktop.loadingDetails[key] && !desktop.detailErrors[key]) {
      void fetchWindowDetail(instanceId, runtimeKey, record.bundleId, record.uri);
    }
  };
  const options = [...new Map(desktop.windows.map((record) => [record.bundleId, record.server]))]
    .map(([value, label]) => ({ value, label }));
  const term = search.trim().toLowerCase();
  const windows = desktop.windows.filter((record) =>
    (sources === undefined || sources.includes(record.bundleId)) &&
    [record.title, record.uri, record.server, record.description ?? ''].some((value) => value.toLowerCase().includes(term)));
  const columns = [
    {
      title: t('desktop.windowTitle'),
      key: 'title',
      width: '33%',
      render: (_: unknown, record: DesktopWindow) => (
        <Button type="link" style={{ padding: 0, maxWidth: '100%', textAlign: 'left', whiteSpace: 'normal', height: 'auto' }}
          onClick={() => openWindow(record)}>{record.title || record.uri}</Button>
      ),
    },
    { title: t('desktop.windowUri'), dataIndex: 'uri', key: 'uri', ellipsis: true, width: '32%' },
    { title: t('desktop.sourceServer'), dataIndex: 'server', key: 'server', width: '20%' },
    {
      title: t('desktop.contentType'), dataIndex: 'mime_type', key: 'mime_type', width: '15%',
      render: (mimeType?: string) => mimeType || t('desktop.unknownContentType'),
    },
  ];

  if (!desktop.loaded && !desktop.loading) {
    return <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('desktop.notLoaded')} />;
  }
  if (desktop.loaded && desktop.windows.length === 0 && !desktop.loading) {
    return desktop.enumerationStatus === 'unverified'
      ? <Alert type="warning" showIcon message={t('desktop.enumerationIndeterminate')} description={t('desktop.enumerationIndeterminateDescription')} />
      : <Empty description={t('desktop.noWindows')} />;
  }

  const selectedKey = selected && desktopWindowKey(selected);
  const detail = selectedKey ? desktop.windowDetails[selectedKey] : undefined;
  const loading = selectedKey ? desktop.loadingDetails[selectedKey] : false;
  const error = selectedKey ? desktop.detailErrors[selectedKey] : undefined;
  const reloadWindow = () => {
    if (selected) void fetchWindowDetail(instanceId, runtimeKey, selected.bundleId, selected.uri);
  };

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space wrap>
        <Input allowClear prefix={<SearchOutlined />} aria-label={t('desktop.searchWindows')}
          placeholder={t('desktop.searchWindows')} value={search} onChange={(event) => setSearch(event.target.value)} />
        {options.length > 0 && <McpSourceFilter options={options} selection={sources} onChange={setSources} />}
      </Space>
      <Table
        columns={columns}
        dataSource={windows}
        rowKey={desktopWindowKey}
        loading={desktop.loading}
        size="middle"
        pagination={false}
        locale={{ emptyText: t('desktop.noMatchingWindows') }}
      />
      <PageModal title={selected?.title || selected?.uri} open={Boolean(selected)} width={760}
        onCancel={() => setSelection(null)} footer={[
          <Button key="refresh" icon={<ReloadOutlined />} onClick={reloadWindow} loading={Boolean(loading)}>{t('common.refresh')}</Button>,
          <Button key="close" type="primary" onClick={() => setSelection(null)}>{t('common.close')}</Button>,
        ]}>
        {selected && <Text type="secondary">{selected.server} · {selected.uri}</Text>}
        <div style={{ marginTop: 12, maxHeight: '60vh', overflow: 'auto' }}>
          {error ? <Alert type="error" showIcon message={t('desktop.failedToLoadDetail')} description={error}
            action={<Button size="small" onClick={reloadWindow}>{t('common.retry')}</Button>} />
            : loading || !detail ? <Spin /> : <WindowContentPreview contents={detail.contents} />}
        </div>
      </PageModal>
      {enumerationNotice.visible && (
        <NoticeBar dismiss={{ label: t('common.dismissNotice'), onClick: enumerationNotice.dismiss }}>
          <Text strong>{t('desktop.enumerationUnverified')}</Text>{' '}
          <Text type="secondary">{t('desktop.enumerationUnverifiedDescription')}</Text>
        </NoticeBar>
      )}
    </Space>
  );
}
