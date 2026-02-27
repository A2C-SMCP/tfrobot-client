import { useEffect } from 'react';
import { Card, Col, Row, Tag, Typography, List, Spin, Button } from 'antd';
import {
  CloudServerOutlined,
  ApiOutlined,
  ToolOutlined,
  CodeOutlined,
  FileTextOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import dayjs from 'dayjs';
import { useDashboardStore } from '@/stores/dashboardStore';

const { Title, Text } = Typography;

interface DashboardProps {
  onNavigate: (key: string) => void;
}

export function Dashboard({ onNavigate }: DashboardProps) {
  const { t } = useTranslation();
  const { data, loading, fetchDashboard } = useDashboardStore();

  useEffect(() => {
    fetchDashboard();
  }, []);

  if (loading && !data) {
    return <Spin style={{ display: 'block', marginTop: 100, textAlign: 'center' }} />;
  }

  if (!data) return null;

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('nav.dashboard')}</Title>
        <Button icon={<ReloadOutlined />} onClick={fetchDashboard}>{t('common.refresh')}</Button>
      </div>

      <Row gutter={[16, 16]}>
        {/* Connection Card */}
        <Col xs={24} sm={12} lg={8}>
          <Card
            hoverable
            onClick={() => onNavigate('smcp')}
            title={<><CloudServerOutlined /> {t('dashboard.connection')}</>}
          >
            <Tag color={data.connected ? 'green' : 'default'}>
              {data.connected ? t('connection.connected') : t('connection.disconnected')}
            </Tag>
            {data.connection_profile && (
              <Text type="secondary" style={{ display: 'block', marginTop: 8 }}>
                {data.connection_profile} — {data.connection_url}
              </Text>
            )}
          </Card>
        </Col>

        {/* MCP Card */}
        <Col xs={24} sm={12} lg={8}>
          <Card
            hoverable
            onClick={() => onNavigate('mcp')}
            title={<><ApiOutlined /> {t('dashboard.mcpServers')}</>}
          >
            <Text>{t('dashboard.total')}: {data.mcp_total}</Text>
            <br />
            <Tag color="green">{t('dashboard.running')}: {data.mcp_running}</Tag>
            <Tag color="default">{t('dashboard.stopped')}: {data.mcp_stopped}</Tag>
          </Card>
        </Col>

        {/* Tools Card */}
        <Col xs={24} sm={12} lg={8}>
          <Card
            hoverable
            onClick={() => onNavigate('debug')}
            title={<><ToolOutlined /> {t('dashboard.tools')}</>}
          >
            <Text style={{ fontSize: 24, fontWeight: 600 }}>{data.tools_count}</Text>
            <Text type="secondary" style={{ marginLeft: 8 }}>{t('dashboard.toolsAvailable')}</Text>
          </Card>
        </Col>

        {/* Runtimes Card */}
        <Col xs={24} sm={12} lg={8}>
          <Card title={<><CodeOutlined /> {t('dashboard.runtimes')}</>}>
            {data.runtimes.map((rt) => (
              <div key={rt.name} style={{ marginBottom: 4 }}>
                <Tag color={rt.available ? 'green' : 'red'}>{rt.name}</Tag>
                {rt.path && <Text type="secondary" style={{ fontSize: 12 }}>{rt.path}</Text>}
              </div>
            ))}
          </Card>
        </Col>

        {/* Recent Activity Card */}
        <Col xs={24} lg={16}>
          <Card
            hoverable
            onClick={() => onNavigate('logs')}
            title={<><FileTextOutlined /> {t('dashboard.recentActivity')}</>}
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
        </Col>
      </Row>
    </div>
  );
}
