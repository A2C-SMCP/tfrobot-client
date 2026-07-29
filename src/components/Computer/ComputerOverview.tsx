import { useEffect } from 'react';
import { Alert, Button, Card, Col, Descriptions, List, Row, Space, Spin, Statistic, Tag, Typography } from 'antd';
import {
  ApiOutlined,
  CloudServerOutlined,
  FileTextOutlined,
  ReloadOutlined,
  ToolOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { useTranslation } from 'react-i18next';
import { useComputerOverviewStore } from '@/stores/computerOverviewStore';
import type { ComputerDetailTab, ComputerSettingsSection } from './tabs';
import { affectedCapabilityLabel } from './runtimeProblemPresentation';

const { Text } = Typography;

interface ComputerOverviewProps {
  instanceId: string;
  onOpenTab: (tab: ComputerDetailTab) => void;
  onOpenSettings: (section: ComputerSettingsSection) => void;
}

export function ComputerOverview({
  instanceId,
  onOpenTab,
  onOpenSettings,
}: ComputerOverviewProps) {
  const { t } = useTranslation();
  const { data, loading, error, fetchOverview } = useComputerOverviewStore();

  useEffect(() => {
    fetchOverview(instanceId);
  }, [fetchOverview, instanceId]);

  if (loading && (!data || data.id !== instanceId)) {
    return <Spin style={{ display: 'block', marginTop: 48, textAlign: 'center' }} />;
  }

  if (error) {
    return <Text type="danger">{error}</Text>;
  }

  if (!data || data.id !== instanceId) {
    return null;
  }
  const currentProblem = (data.runtime.problems ?? [])
    .find((problem) => problem.current && problem.severity === 'error')
    ?? (data.runtime.problems ?? []).find((problem) => problem.current);

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <Button icon={<ReloadOutlined />} onClick={() => fetchOverview(instanceId)}>
          {t('common.refresh')}
        </Button>
      </div>

      {currentProblem && (
        <Alert
          type={currentProblem.severity === 'error' ? 'error' : 'warning'}
          showIcon
          message={t(`computer.runtime.problems.messages.${currentProblem.message}`)}
          description={t('computer.runtime.problems.affected', {
            capabilities: currentProblem.affected_capabilities
              .map((capability) => affectedCapabilityLabel(capability, t))
              .join(', '),
          })}
        />
      )}

      <Row gutter={[16, 16]}>
        <Col xs={24}>
          <Card title={t('computer.runtime.statusTitle')}>
            <Descriptions column={{ xs: 1, sm: 3 }} size="small">
              <Descriptions.Item label={t('computer.runtime.userState')}>
                <Tag color={data.runtime.user_state === 'error'
                  ? 'red'
                  : data.runtime.user_state === 'degraded'
                    ? 'orange'
                    : data.running
                      ? 'green'
                      : 'default'}
                >
                  {t(`computer.runtime.userStates.${data.runtime.user_state}`)}
                </Tag>
              </Descriptions.Item>
              <Descriptions.Item label={t('skills.title')}>
                {data.runtime.skills}
              </Descriptions.Item>
            </Descriptions>
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card title={<><CloudServerOutlined /> {t('dashboard.connection')}</>}>
            <Tag color={data.connected ? 'green' : 'default'}>
              {data.connected ? t('connection.connected') : t('connection.disconnected')}
            </Tag>
            {data.connection_profile && (
              <Text type="secondary" style={{ display: 'block', marginTop: 8 }}>
                {data.connection_profile} — {data.connection_url}
              </Text>
            )}
            {data.robot_name && (
              <Text type="secondary" style={{ display: 'block', marginTop: 8 }}>
                {t('computer.boundRobot', { name: data.robot_name })}
              </Text>
            )}
            <Button type="link" style={{ paddingLeft: 0 }} onClick={() => onOpenSettings('connection')}>
              {t('computer.robotConnection')}
            </Button>
          </Card>
        </Col>

        <Col xs={24} sm={12} lg={6}>
          <Card title={<><ApiOutlined /> {t('dashboard.mcpServers')}</>}>
            <Statistic value={data.mcp_total} />
            <Space wrap style={{ marginTop: 8 }}>
              <Tag color="green">{t('dashboard.running')}: {data.mcp_running}</Tag>
              <Tag color="default">{t('dashboard.stopped')}: {data.mcp_stopped}</Tag>
            </Space>
            <Button type="link" style={{ paddingLeft: 0 }} onClick={() => onOpenSettings('mcp')}>
              {t('mcp.servers')}
            </Button>
          </Card>
        </Col>

        <Col xs={24} sm={12} lg={6}>
          <Card title={<><ToolOutlined /> {t('dashboard.tools')}</>}>
            <Statistic value={data.tools_count} />
            <Text type="secondary">{t('dashboard.toolsAvailable')}</Text>
            <br />
            <Button type="link" style={{ paddingLeft: 0 }} onClick={() => onOpenTab('debug')}>
              {t('nav.debugPanel')}
            </Button>
          </Card>
        </Col>

        <Col xs={24} sm={12} lg={6}>
          <Card title={<><FileTextOutlined /> {t('dashboard.recentActivity')}</>}>
            <Statistic value={data.recent_logs.length} />
            <Text type="secondary">{t('logs.title')}</Text>
            <br />
            <Button type="link" style={{ paddingLeft: 0 }} onClick={() => onOpenTab('logs')}>
              {t('logs.title')}
            </Button>
          </Card>
        </Col>

        <Col xs={24}>
          <Card title={<><FileTextOutlined /> {t('dashboard.recentActivity')}</>}>
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
    </Space>
  );
}
