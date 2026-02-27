import { useEffect } from 'react';
import { Button, Space, Typography, Table, Empty, Alert } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useDesktopStore } from '@/stores/desktopStore';

const { Title } = Typography;

export function DesktopResources() {
  const { t } = useTranslation();
  const { windows, loading, error, fetchDesktop } = useDesktopStore();

  useEffect(() => {
    fetchDesktop();
  }, [fetchDesktop]);

  const columns = [
    {
      title: t('desktop.windowUri'),
      dataIndex: 'uri',
      key: 'uri',
      ellipsis: true,
    },
    {
      title: t('desktop.windowTitle'),
      dataIndex: 'title',
      key: 'title',
    },
    {
      title: t('desktop.sourceServer'),
      dataIndex: 'server',
      key: 'server',
    },
  ];

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('desktop.title')}</Title>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={() => fetchDesktop()} loading={loading}>
            {t('common.refresh')}
          </Button>
        </Space>
      </div>

      {error && (
        <Alert message={t('common.error')} description={error} type="error" showIcon closable style={{ marginBottom: 16 }} />
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
        />
      )}
    </div>
  );
}
