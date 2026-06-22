import { Button, Card, Col, Empty, Row, Skeleton, Space, Tabs, Tag, Typography } from 'antd';
import {
  ApiOutlined,
  BugOutlined,
  CloudServerOutlined,
  DesktopOutlined,
  FormOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
import { useComputerStore, type ComputerInstance, type ComputerStatus } from '@/stores/computerStore';
import { McpConfig } from '@/components/McpConfig';
import { InputVariables } from '@/components/InputVariables';
import { SmcpConnection } from '@/components/SmcpConnection';
import { DesktopResources } from '@/components/DesktopResources';
import { DebugPanel } from '@/components/DebugPanel';
import { RuntimeSettings } from '@/components/Settings/RuntimeSettings';
import { ManagerAccount } from '@/components/ManagerAccount';
import { toComputerDetailTab, type ComputerDetailTab } from './tabs';

const { Title, Text } = Typography;

const statusColor: Record<ComputerStatus, string> = {
  running: 'success',
  stopped: 'default',
  error: 'error',
};

interface ComputerProps {
  initialView?: 'list' | 'detail';
  initialTab?: ComputerDetailTab;
}

function ComputerCard({
  instance,
  onOpen,
}: {
  instance: ComputerInstance;
  onOpen: () => void;
}) {
  const { t } = useTranslation();

  return (
    <Card
      title={
        <Space>
          <DesktopOutlined />
          <span>{instance.name}</span>
        </Space>
      }
      extra={
        <Tag color={statusColor[instance.status]}>
          {t(`computer.status.${instance.status}`)}
        </Tag>
      }
      actions={[
        <Button key="open" type="link" onClick={onOpen}>
          {t('computer.openDetails')}
        </Button>,
      ]}
    >
      <Space direction="vertical" size={8} style={{ width: '100%' }}>
        <Space wrap>
          <Tag color={instance.connectionStatus === 'connected' ? 'green' : 'default'}>
            {t(`computer.connection.${instance.connectionStatus}`)}
          </Tag>
          <Tag icon={<ApiOutlined />}>
            {t('computer.mcpServers', { count: instance.mcpServerCount })}
          </Tag>
        </Space>
        <Text type="secondary">
          {instance.robotName
            ? t('computer.boundRobot', { name: instance.robotName })
            : t('computer.noRobotBound')}
        </Text>
        {instance.connectionProfile && (
          <Text type="secondary">
            {t('computer.connectionProfile', { name: instance.connectionProfile })}
          </Text>
        )}
      </Space>
    </Card>
  );
}

export function Computer({ initialView = 'list', initialTab = 'mcp' }: ComputerProps) {
  const { t } = useTranslation();
  const { instances, loading, selectedInstanceId, fetchInstances, selectInstance } = useComputerStore();
  const [view, setView] = useState<'list' | 'detail'>(initialView);
  const [activeTab, setActiveTab] = useState<ComputerDetailTab>(initialTab);

  useEffect(() => {
    fetchInstances();
  }, [fetchInstances]);

  useEffect(() => {
    setView(initialView);
    setActiveTab(initialTab);
  }, [initialView, initialTab]);

  const selectedInstance = instances.find((instance) => instance.id === selectedInstanceId) ?? instances[0];
  const showDetail = view === 'detail' && selectedInstance;

  if (loading && instances.length === 0) {
    return <Skeleton active paragraph={{ rows: 4 }} />;
  }

  if (showDetail) {
    return (
      <div>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', gap: 16, alignItems: 'flex-start' }}>
            <div>
              <Title level={4} style={{ margin: 0 }}>{selectedInstance.name}</Title>
              <Space wrap style={{ marginTop: 8 }}>
                <Tag color={statusColor[selectedInstance.status]}>
                  {t(`computer.status.${selectedInstance.status}`)}
                </Tag>
                <Tag color={selectedInstance.connectionStatus === 'connected' ? 'green' : 'default'}>
                  {t(`computer.connection.${selectedInstance.connectionStatus}`)}
                </Tag>
                <Text type="secondary">
                  {selectedInstance.robotName
                    ? t('computer.boundRobot', { name: selectedInstance.robotName })
                    : t('computer.noRobotBound')}
                </Text>
                {selectedInstance.connectionProfile && (
                  <Text type="secondary">
                    {t('computer.connectionProfile', { name: selectedInstance.connectionProfile })}
                  </Text>
                )}
              </Space>
            </div>
            <Button onClick={() => setView('list')}>
              {t('computer.backToList')}
            </Button>
          </div>

          <Tabs
            activeKey={activeTab}
            onChange={(key) => setActiveTab(toComputerDetailTab(key))}
            items={[
              { key: 'mcp', label: <><ApiOutlined /> {t('mcp.servers')}</>, children: <McpConfig instanceId={selectedInstance.id} /> },
              { key: 'inputs', label: <><FormOutlined /> {t('inputs.title')}</>, children: <InputVariables instanceId={selectedInstance.id} /> },
              { key: 'connection', label: <><CloudServerOutlined /> {t('computer.robotConnection')}</>, children: (
                <Space direction="vertical" size={16} style={{ width: '100%' }}>
                  <ManagerAccount />
                  <SmcpConnection instanceId={selectedInstance.id} />
                </Space>
              ) },
              { key: 'resources', label: <><DesktopOutlined /> {t('resources.title')}</>, children: <DesktopResources instanceId={selectedInstance.id} /> },
              { key: 'debug', label: <><BugOutlined /> {t('nav.debugPanel')}</>, children: <DebugPanel instanceId={selectedInstance.id} /> },
              { key: 'runtime', label: <><SettingOutlined /> {t('settings.runtime')}</>, children: <RuntimeSettings /> },
            ]}
          />
        </Space>
      </div>
    );
  }

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('computer.title')}</Title>
        <Button type="primary" disabled>
          {t('computer.create')}
        </Button>
      </div>

      {instances.length === 0 ? (
        <Empty description={t('computer.empty')} />
      ) : (
        <Row gutter={[16, 16]}>
          {instances.map((instance) => (
            <Col key={instance.id} xs={24} lg={12} xl={8}>
              <ComputerCard
                instance={instance}
                onOpen={() => {
                  selectInstance(instance.id);
                  setView('detail');
                }}
              />
            </Col>
          ))}
        </Row>
      )}
    </div>
  );
}
