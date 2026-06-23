import { useEffect } from 'react';
import { Button, Card, Col, Divider, List, Row, Space, Spin, Statistic, Tag, Typography } from 'antd';
import {
  CodeOutlined,
  DesktopOutlined,
  FileTextOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import dayjs from 'dayjs';
import { useDashboardStore } from '@/stores/dashboardStore';
import { useComputerStore } from '@/stores/computerStore';

const { Title, Text } = Typography;

interface DashboardProps {
  onNavigate: (key: string) => void;
}

export function Dashboard({ onNavigate }: DashboardProps) {
  const { t } = useTranslation();
  const { data, loading, fetchDashboard } = useDashboardStore();
  const { selectInstance } = useComputerStore();

  useEffect(() => {
    fetchDashboard();
  }, []);

  if (loading && !data) {
    return <Spin style={{ display: 'block', marginTop: 100, textAlign: 'center' }} />;
  }

  if (!data) return null;

  const openComputer = (id: string) => {
    selectInstance(id);
    onNavigate('computer-detail:overview');
  };

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('nav.dashboard')}</Title>
        <Button icon={<ReloadOutlined />} onClick={fetchDashboard}>{t('common.refresh')}</Button>
      </div>

      <Space direction="vertical" size={16} style={{ width: '100%' }}>
        <Card
          title={<><DesktopOutlined /> {t('dashboard.computers')}</>}
          styles={{ body: { paddingTop: 20 } }}
        >
          <Row gutter={[24, 16]} style={{ marginBottom: 16 }}>
            <Col xs={12} sm={6}>
              <Statistic title={t('dashboard.computers')} value={data.computer_total} />
            </Col>
            <Col xs={12} sm={6}>
              <Statistic title={t('dashboard.running')} value={data.computer_running} valueStyle={{ color: '#52c41a' }} />
            </Col>
            <Col xs={12} sm={6}>
              <Statistic title={t('dashboard.stopped')} value={data.computer_stopped} />
            </Col>
            <Col xs={12} sm={6}>
              <Statistic title={t('connection.connected')} value={data.computer_connected} valueStyle={{ color: '#1677ff' }} />
            </Col>
          </Row>

          <Divider style={{ margin: '12px 0' }} />

          <List
            dataSource={data.computers}
            locale={{ emptyText: t('computer.empty') }}
            renderItem={(computer) => (
              <List.Item
                style={{ padding: '14px 0' }}
                actions={[
                  <Button
                    key="open"
                    onClick={() => openComputer(computer.id)}
                  >
                    {t('computer.openDetails')}
                  </Button>,
                ]}
              >
                <List.Item.Meta
                  title={computer.name}
                  description={(
                    <Space wrap size={[8, 8]}>
                      <Tag color={computer.running ? 'green' : 'default'}>
                        {computer.running ? t('computer.status.running') : t('computer.status.stopped')}
                      </Tag>
                      <Tag color={computer.connected ? 'green' : 'default'}>
                        {computer.connected ? t('connection.connected') : t('connection.disconnected')}
                      </Tag>
                      <Tag>{t('computer.mcpServers', { count: computer.mcp_server_count })}</Tag>
                      {computer.robot_name && <Text type="secondary">{computer.robot_name}</Text>}
                      {computer.connection_profile && <Text type="secondary">{computer.connection_profile}</Text>}
                    </Space>
                  )}
                />
              </List.Item>
            )}
          />
        </Card>

        <Card title={<><CodeOutlined /> {t('dashboard.runtimes')}</>}>
          <Space direction="vertical" size={8} style={{ width: '100%' }}>
            {data.runtimes.map((rt) => (
              <div key={rt.name} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <Tag color="default" style={{ minWidth: 72, textAlign: 'center' }}>
                  {rt.name}
                </Tag>
                <Tag color={rt.available ? 'green' : 'red'}>
                  {rt.available ? t('dashboard.runtimeAvailable') : t('dashboard.runtimeUnavailable')}
                </Tag>
                {rt.path && (
                  <Text type="secondary" ellipsis style={{ flex: 1, fontSize: 12 }}>
                    {rt.path}
                  </Text>
                )}
              </div>
            ))}
          </Space>
        </Card>

        <Card
          title={<><FileTextOutlined /> {t('dashboard.recentActivity')}</>}
          extra={<Button type="link" onClick={() => onNavigate('logs')}>{t('logs.title')}</Button>}
        >
          <List
            size="small"
            dataSource={data.recent_logs}
            locale={{ emptyText: t('dashboard.noActivity') }}
            renderItem={(log) => (
                <List.Item>
                  <Text type="secondary" style={{ marginRight: 8, fontSize: 12 }}>
                    {dayjs(log.timestamp).format('HH:mm:ss')}
                  </Text>
                  <Tag color={log.level === 'error' ? 'red' : log.level === 'warn' ? 'orange' : 'blue'}>
                    {log.level}
                  </Tag>
                  <Text>{log.message}</Text>
                </List.Item>
            )}
          />
        </Card>
      </Space>
    </div>
  );
}
