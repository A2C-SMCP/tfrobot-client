import { Space, Table, Tag, Typography } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { useTranslation } from 'react-i18next';
import { NOTICE_IDS, useUiNoticeStore } from '@/stores/uiNoticeStore';

const { Text } = Typography;

interface NoticeRow {
  id: string;
  label: string;
  impressions: number;
  dismissed: boolean;
  helpClicks: number;
}

function rate(numerator: number, denominator: number): string {
  return denominator === 0 ? '—' : `${Math.round((numerator / denominator) * 100)}%`;
}

/**
 * Read-only view of the local notice counters. These numbers describe how often each notice was
 * shown and how often users acted on it, which is how the team decides whether a notice still earns
 * the space it takes. Nothing here is reported anywhere.
 */
export function NoticeStats() {
  const { t } = useTranslation();
  const entries = useUiNoticeStore((state) => state.entries);

  const rows: NoticeRow[] = NOTICE_IDS.map((id) => {
    const entry = entries[id];
    return {
      id,
      label: t(`debug.notices.${id}`),
      impressions: entry?.impressions ?? 0,
      dismissed: Boolean(entry?.dismissedAt),
      helpClicks: entry?.helpClicks ?? 0,
    };
  });

  const columns: ColumnsType<NoticeRow> = [
    { title: t('debug.noticeColumns.notice'), dataIndex: 'label', key: 'label' },
    { title: t('debug.noticeColumns.impressions'), dataIndex: 'impressions', key: 'impressions', width: 120 },
    {
      title: t('debug.noticeColumns.dismissed'),
      dataIndex: 'dismissed',
      key: 'dismissed',
      width: 110,
      render: (dismissed: boolean) => (
        <Tag color={dismissed ? 'default' : 'blue'}>
          {t(dismissed ? 'common.yes' : 'common.no')}
        </Tag>
      ),
    },
    { title: t('debug.noticeColumns.helpClicks'), dataIndex: 'helpClicks', key: 'helpClicks', width: 110 },
    {
      title: t('debug.noticeColumns.dismissRate'),
      key: 'dismissRate',
      width: 110,
      render: (_, row) => rate(row.dismissed ? 1 : 0, row.impressions),
    },
    {
      title: t('debug.noticeColumns.helpRate'),
      key: 'helpRate',
      width: 130,
      render: (_, row) => rate(row.helpClicks, row.impressions),
    },
  ];

  return (
    <Space direction="vertical" size={8} style={{ width: '100%' }}>
      <Text type="secondary">{t('debug.noticesLocalOnly')}</Text>
      <Table
        columns={columns}
        dataSource={rows}
        pagination={false}
        rowKey="id"
        size="small"
      />
    </Space>
  );
}
