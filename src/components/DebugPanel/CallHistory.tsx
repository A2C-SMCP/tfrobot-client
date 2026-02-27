import { useEffect } from 'react';
import { Table, Tag, Typography, Empty, Button, Space, Descriptions } from 'antd';
import { ReloadOutlined, PlayCircleOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useDebugStore } from '@/stores/debugStore';

const { Text } = Typography;

export function CallHistory() {
  const { t } = useTranslation();
  const { history, historyLoading, fetchHistory, selectTool, tools } = useDebugStore();

  useEffect(() => {
    fetchHistory();
  }, [fetchHistory]);

  const handleReplay = (record: (typeof history)[0]) => {
    const tool = tools.find((t) => t.name === record.tool);
    if (tool) {
      selectTool(tool);
    }
  };

  const columns = [
    {
      title: t('debug.historyColumns.time'),
      dataIndex: 'timestamp',
      key: 'timestamp',
      width: 180,
      render: (ts: string) => new Date(ts).toLocaleString(),
    },
    {
      title: t('debug.historyColumns.tool'),
      dataIndex: 'tool',
      key: 'tool',
      ellipsis: true,
    },
    {
      title: t('debug.historyColumns.server'),
      dataIndex: 'server',
      key: 'server',
      width: 120,
      render: (s: string) => <Tag>{s}</Tag>,
    },
    {
      title: t('debug.historyColumns.status'),
      dataIndex: 'success',
      key: 'success',
      width: 80,
      render: (success: boolean) => (
        <Tag color={success ? 'success' : 'error'}>
          {success ? '✓' : '✗'}
        </Tag>
      ),
    },
  ];

  return (
    <div>
      <Space style={{ marginBottom: 16 }}>
        <Button icon={<ReloadOutlined />} onClick={() => fetchHistory()} loading={historyLoading}>
          {t('common.refresh')}
        </Button>
      </Space>

      {history.length === 0 && !historyLoading ? (
        <Empty description={t('debug.noHistory')} />
      ) : (
        <Table
          columns={columns}
          dataSource={history}
          rowKey="req_id"
          loading={historyLoading}
          size="small"
          pagination={{ pageSize: 20 }}
          expandable={{
            expandedRowRender: (record) => (
              <div>
                <Descriptions size="small" column={1}>
                  <Descriptions.Item label={t('debug.historyColumns.params')}>
                    <pre style={{ margin: 0, fontSize: 12 }}>
                      {JSON.stringify(record.parameters, null, 2)}
                    </pre>
                  </Descriptions.Item>
                  {record.error && (
                    <Descriptions.Item label={t('common.error')}>
                      <Text type="danger">{record.error}</Text>
                    </Descriptions.Item>
                  )}
                </Descriptions>
                <Button
                  size="small"
                  icon={<PlayCircleOutlined />}
                  onClick={() => handleReplay(record)}
                  style={{ marginTop: 8 }}
                >
                  {t('debug.replay')}
                </Button>
              </div>
            ),
          }}
        />
      )}
    </div>
  );
}
