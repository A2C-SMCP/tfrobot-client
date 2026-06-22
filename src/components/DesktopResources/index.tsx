import { useEffect, useState } from 'react';
import { App, Button, Space, Typography, Table, Empty, Alert, Spin, Image } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useDesktopStore, WindowContent } from '@/stores/desktopStore';

const { Title, Text } = Typography;

// Constants
const MAX_TEXT_PREVIEW_LENGTH = 500;

interface DesktopResourcesProps {
  instanceId: string;
}

export function DesktopResources({ instanceId }: DesktopResourcesProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    windows,
    loading,
    error,
    fetchDesktop,
    windowDetails,
    loadingDetails,
    detailErrors,
    fetchWindowDetail,
  } = useDesktopStore();
  const [expandedUris, setExpandedUris] = useState<Set<string>>(new Set());

  useEffect(() => {
    fetchDesktop(instanceId);
  }, [fetchDesktop, instanceId]);

  // Show error message when detail fetch fails
  useEffect(() => {
    Object.entries(detailErrors).forEach(([_uri, err]) => {
      if (err) {
        message.error(`${t('desktop.failedToLoadDetail')}: ${err}`);
      }
    });
  }, [detailErrors, t, message]);

  const handleRowExpand = async (expanded: boolean, record: { uri: string; server: string }) => {
    const newExpanded = new Set(expandedUris);
    if (!expanded) {
      newExpanded.delete(record.uri);
      setExpandedUris(newExpanded);
    } else {
      newExpanded.add(record.uri);
      setExpandedUris(newExpanded);
      await fetchWindowDetail(instanceId, record.server, record.uri);
    }
  };

  const renderContent = (contents: WindowContent[]) => {
    if (contents.length === 0) {
      return <Text type="secondary">{t('desktop.noContent')}</Text>;
    }

    return contents.map((content, idx) => (
      <div key={idx} style={{ marginBottom: 8 }}>
        {content.type === 'text' && content.text && (
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {content.mime_type || 'text'}
            </Text>
            <div
              style={{
                backgroundColor: '#f5f5f5',
                padding: '8px 12px',
                borderRadius: 4,
                marginTop: 4,
                maxWidth: '100%',
                overflow: 'auto',
              }}
            >
              <Text
                code
                style={{
                  wordBreak: 'break-all',
                  whiteSpace: 'pre-wrap',
                  fontSize: 12,
                }}
              >
                {content.text.length > MAX_TEXT_PREVIEW_LENGTH
                  ? content.text.slice(0, MAX_TEXT_PREVIEW_LENGTH) + '...'
                  : content.text}
              </Text>
            </div>
          </div>
        )}
        {content.type === 'blob' && content.mime_type?.startsWith('image/') && content.blob && (
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {content.mime_type}
            </Text>
            <div style={{ marginTop: 8 }}>
              <Image
                src={`data:${content.mime_type};base64,${content.blob}`}
                alt="Window screenshot"
                style={{ maxWidth: 400, maxHeight: 300, borderRadius: 4 }}
                placeholder={<Spin />}
              />
            </div>
          </div>
        )}
        {content.type === 'blob' && !content.mime_type?.startsWith('image/') && (
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {content.mime_type || 'binary'}
            </Text>
            <div style={{ marginTop: 4 }}>
              <Text type="secondary">
                [{t('desktop.binaryData')}: {Math.floor((content.blob?.length || 0) * 3 / 4)} bytes]
              </Text>
            </div>
          </div>
        )}
      </div>
    ));
  };

  const columns = [
    {
      title: t('desktop.windowUri'),
      dataIndex: 'uri',
      key: 'uri',
      ellipsis: true,
      width: '40%',
    },
    {
      title: t('desktop.windowTitle'),
      dataIndex: 'title',
      key: 'title',
      width: '30%',
    },
    {
      title: t('desktop.sourceServer'),
      dataIndex: 'server',
      key: 'server',
      width: '30%',
    },
  ];

  return (
    <div>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 16,
        }}
      >
        <Title level={4} style={{ margin: 0 }}>
          {t('desktop.title')}
        </Title>
        <Space>
          <Button
            icon={<ReloadOutlined />}
            onClick={() => fetchDesktop(instanceId)}
            loading={loading}
          >
            {t('common.refresh')}
          </Button>
        </Space>
      </div>

      {error && (
        <Alert
          message={t('common.error')}
          description={error}
          type="error"
          showIcon
          closable
          style={{ marginBottom: 16 }}
        />
      )}

      {windows.length === 0 && !loading ? (
        <Empty description={t('desktop.noWindows')} />
      ) : (
        <Table
          columns={columns}
          dataSource={windows}
          rowKey="uri"
          loading={loading}
          size="middle"
          pagination={false}
          expandable={{
            expandedRowKeys: Array.from(expandedUris),
            onExpand: handleRowExpand,
            expandedRowRender: (record) => {
              const detail = windowDetails[record.uri];
              const isLoading = loadingDetails[record.uri];

              if (isLoading) {
                return (
                  <div style={{ padding: '16px 0', textAlign: 'center' }}>
                    <Spin />
                  </div>
                );
              }

              if (!detail) {
                return (
                  <Text type="secondary">{t('desktop.clickToLoadDetail')}</Text>
                );
              }

              return (
                <div style={{ padding: '12px 0' }}>
                  <div
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'center',
                      marginBottom: 12,
                    }}
                  >
                    <Title level={5} style={{ margin: 0 }}>
                      {t('desktop.windowContent')}
                    </Title>
                    <Button
                      size="small"
                      icon={<ReloadOutlined />}
                      onClick={() => fetchWindowDetail(instanceId, record.server, record.uri)}
                      loading={isLoading}
                    />
                  </div>
                  {renderContent(detail.contents)}
                </div>
              );
            },
            rowExpandable: () => true,
          }}
        />
      )}
    </div>
  );
}
