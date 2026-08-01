import { useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Button,
  Empty,
  Space,
  Spin,
  Table,
  Typography,
} from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  desktopWindowKey,
  type DesktopInstanceState,
  type DesktopWindow,
} from '@/stores/desktopStore';
import { WindowContentPreview } from './WindowContentPreview';

const { Text } = Typography;

interface DesktopResourcesTableProps {
  desktop: DesktopInstanceState;
  instanceId: string;
  runtimeKey: string;
  fetchWindowDetail: (
    instanceId: string,
    runtimeKey: string,
    bundleId: string,
    uri: string,
  ) => Promise<void>;
}

export function DesktopResourcesTable({
  desktop,
  instanceId,
  runtimeKey,
  fetchWindowDetail,
}: DesktopResourcesTableProps) {
  const { t } = useTranslation();
  const [expandedUris, setExpandedUris] = useState<Set<string>>(new Set());

  useEffect(() => {
    setExpandedUris(new Set());
  }, [instanceId, runtimeKey, desktop.windows]);

  const columns = useMemo(() => [
    {
      title: t('desktop.windowUri'),
      dataIndex: 'uri',
      key: 'uri',
      ellipsis: true,
      width: '32%',
    },
    {
      title: t('desktop.windowTitle'),
      dataIndex: 'title',
      key: 'title',
      width: '24%',
    },
    {
      title: t('desktop.sourceServer'),
      dataIndex: 'server',
      key: 'server',
      width: '24%',
    },
    {
      title: t('desktop.contentType'),
      dataIndex: 'mime_type',
      key: 'mime_type',
      width: '20%',
      render: (mimeType?: string) => mimeType || t('desktop.unknownContentType'),
    },
  ], [t]);

  const handleRowExpand = (expanded: boolean, record: DesktopWindow) => {
    const key = desktopWindowKey(record);
    setExpandedUris((current) => {
      const next = new Set(current);
      if (expanded) next.add(key);
      else next.delete(key);
      return next;
    });
    if (
      expanded
      && !desktop.windowDetails[key]
      && !desktop.loadingDetails[key]
      && !desktop.detailErrors[key]
    ) {
      void fetchWindowDetail(
        instanceId,
        runtimeKey,
        record.bundleId,
        record.uri,
      );
    }
  };

  if (!desktop.loaded && !desktop.loading) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description={t('desktop.notLoaded')}
      />
    );
  }

  if (desktop.loaded && desktop.windows.length === 0 && !desktop.loading) {
    if (desktop.enumerationStatus === 'unverified') {
      return (
        <Alert
          type="warning"
          showIcon
          message={t('desktop.enumerationIndeterminate')}
          description={t('desktop.enumerationIndeterminateDescription')}
        />
      );
    }
    return <Empty description={t('desktop.noWindows')} />;
  }

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {desktop.enumerationStatus === 'unverified' && desktop.windows.length > 0 && (
        <Alert
          type="info"
          showIcon
          message={t('desktop.enumerationUnverified')}
          description={t('desktop.enumerationUnverifiedDescription')}
        />
      )}
      <Table
        columns={columns}
        dataSource={desktop.windows}
        rowKey={desktopWindowKey}
        loading={desktop.loading}
        size="middle"
        pagination={false}
        expandable={{
          expandedRowKeys: Array.from(expandedUris),
          onExpand: handleRowExpand,
          expandedRowRender: (record) => {
            const key = desktopWindowKey(record);
            const detail = desktop.windowDetails[key];
            const isLoading = desktop.loadingDetails[key];
            const detailError = desktop.detailErrors[key];

            if (isLoading) {
              return (
                <div style={{ padding: '16px 0', textAlign: 'center' }}>
                  <Spin />
                </div>
              );
            }

            if (detailError) {
              return (
                <Alert
                  type="error"
                  showIcon
                  message={t('desktop.failedToLoadDetail')}
                  description={detailError}
                  action={(
                    <Button
                      size="small"
                      onClick={() => {
                        void fetchWindowDetail(
                          instanceId,
                          runtimeKey,
                          record.bundleId,
                          record.uri,
                        );
                      }}
                    >
                      {t('common.retry')}
                    </Button>
                  )}
                />
              );
            }

            if (!detail) {
              return <Text type="secondary">{t('desktop.clickToLoadDetail')}</Text>;
            }

            return (
              <div style={{ padding: '12px 0' }}>
                <Space
                  align="center"
                  style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 12 }}
                >
                  <Text strong>{t('desktop.windowContent')}</Text>
                  <Button
                    size="small"
                    icon={<ReloadOutlined />}
                    aria-label={t('desktop.refreshDetail')}
                    onClick={() => {
                      void fetchWindowDetail(
                        instanceId,
                        runtimeKey,
                        record.bundleId,
                        record.uri,
                      );
                    }}
                  />
                </Space>
                <WindowContentPreview contents={detail.contents} />
              </div>
            );
          },
        }}
      />
    </Space>
  );
}
