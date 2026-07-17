import {
  Alert,
  Button,
  Card,
  Col,
  Descriptions,
  Empty,
  List,
  Row,
  Space,
  Statistic,
  Tag,
  Tooltip,
  Typography,
} from 'antd';
import {
  DisconnectOutlined,
  LinkOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  RetweetOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { McpConfig } from '@/components/McpConfig';
import { isRuntimeRunning, type ComputerInstance } from '@/stores/computerStore';
import {
  useRuntimeStore,
  type ComputerRuntimeEventCause,
  type ComputerRuntimeEventRecord,
} from '@/stores/runtimeStore';

const EMPTY_RUNTIME_EVENTS: ComputerRuntimeEventRecord[] = [];

interface ComputerRuntimeProps {
  instance: ComputerInstance;
  loading: boolean;
  canConnect: boolean;
  connectDisabledReason?: string;
  onStartStop: () => void;
  onRestart: () => void;
  onReload: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
}

export function ComputerRuntime({
  instance,
  loading,
  canConnect,
  connectDisabledReason,
  onStartStop,
  onRestart,
  onReload,
  onConnect,
  onDisconnect,
}: ComputerRuntimeProps) {
  const { t } = useTranslation();
  const runtime = instance.runtime;
  const actions = runtime.actions;
  const recentEvents = useRuntimeStore((state) => state.eventsByInstance[instance.id])
    ?? EMPTY_RUNTIME_EVENTS;
  const running = isRuntimeRunning(runtime);
  const lifecycleColor = runtime.lifecycle === 'error'
    ? 'red'
    : runtime.lifecycle === 'degraded'
      ? 'orange'
        : running
        ? 'green'
        : 'default';
  const describeEventCause = (cause: ComputerRuntimeEventCause): string => {
    switch (cause.kind) {
      case 'lifecycle_changed':
        return t('computer.runtime.eventCauses.lifecycleChanged', {
          state: t(`computer.runtime.lifecycleStates.${cause.state}`),
        });
      case 'config_revision_bumped':
        return t('computer.runtime.eventCauses.configRevisionBumped', {
          revision: cause.revision,
        });
      case 'capability_revision_bumped':
        return t('computer.runtime.eventCauses.capabilityRevisionBumped', {
          revision: cause.revision,
        });
      case 'client_connection_authority_changed':
        return t('computer.runtime.eventCauses.clientConnectionAuthorityChanged', {
          revision: cause.revision,
          status: cause.present
            ? t('computer.runtime.eventCauses.connectionPresent')
            : t('computer.runtime.eventCauses.connectionAbsent'),
        });
      case 'client_diagnostic_changed':
        return t('computer.runtime.eventCauses.clientDiagnosticChanged', {
          operation: cause.operation,
          status: cause.has_error
            ? t('computer.runtime.eventCauses.diagnosticFailed')
            : t('computer.runtime.eventCauses.diagnosticCleared'),
        });
      case 'handle_replaced':
        return t('computer.runtime.eventCauses.handleReplaced', { reason: cause.reason });
      case 'observation_advanced':
        return t('computer.runtime.eventCauses.observationAdvanced');
      case 'resync':
        return t('computer.runtime.eventCauses.resync', { count: cause.skipped_events });
    }
  };

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      {(runtime.last_error || runtime.degraded_reason) && (
        <Alert
          type={runtime.last_error ? 'error' : 'warning'}
          showIcon
          message={runtime.last_error ? t('computer.runtime.lastError') : t('computer.runtime.degraded')}
          description={runtime.last_error ?? runtime.degraded_reason}
        />
      )}

      <Card
        title={t('computer.runtime.statusTitle')}
        extra={(
          <Space wrap>
            <Button
              icon={running ? <StopOutlined /> : <PlayCircleOutlined />}
              disabled={running ? !actions.can_stop : !actions.can_start}
              loading={loading}
              onClick={onStartStop}
            >
              {running ? t('computer.stop') : t('computer.start')}
            </Button>
            <Button
              icon={<RetweetOutlined />}
              disabled={!actions.can_restart}
              loading={loading}
              onClick={onRestart}
            >
              {t('computer.runtime.restart')}
            </Button>
            <Button
              icon={<ReloadOutlined />}
              disabled={!actions.can_reload}
              loading={loading}
              onClick={onReload}
            >
              {t('computer.runtime.reload')}
            </Button>
            {instance.connectionStatus === 'connected' ? (
              <Button
                danger
                icon={<DisconnectOutlined />}
                disabled={!actions.can_disconnect}
                loading={loading}
                onClick={onDisconnect}
              >
                {t('connection.disconnect')}
              </Button>
            ) : (
              <Tooltip title={connectDisabledReason}>
                <Button
                  type="primary"
                  icon={<LinkOutlined />}
                  disabled={!canConnect || !actions.can_connect}
                  loading={loading}
                  onClick={onConnect}
                >
                  {t('connection.connect')}
                </Button>
              </Tooltip>
            )}
          </Space>
        )}
      >
        <Descriptions column={{ xs: 1, sm: 2, lg: 3 }} size="small">
          <Descriptions.Item label={t('computer.runtime.lifecycle')}>
            <Tag color={lifecycleColor}>{t(`computer.runtime.lifecycleStates.${runtime.lifecycle}`)}</Tag>
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.generation')}>
            {runtime.generation}
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.snapshotRevision')}>
            {runtime.snapshot_revision}
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.connection')}>
            <Tag color={instance.connectionStatus === 'connected' ? 'green' : 'default'}>
              {t(`computer.connection.${instance.connectionStatus}`)}
            </Tag>
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.configRevision')}>
            {runtime.config_revision}
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.capabilityRevision')}>
            {runtime.capability_revision}
          </Descriptions.Item>
        </Descriptions>
      </Card>

      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title={t('dashboard.mcpServers')} value={runtime.mcp_servers} /></Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title={t('computer.runtime.activeMcpServers')} value={runtime.active_mcp_servers} /></Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title={t('dashboard.tools')} value={runtime.tools} /></Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small"><Statistic title={t('skills.title')} value={runtime.skills} /></Card>
        </Col>
      </Row>

      <Card title={t('computer.runtime.recentEvents')}>
        {recentEvents.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('computer.runtime.noEvents')} />
        ) : (
          <List
            size="small"
            dataSource={[...recentEvents].reverse()}
            renderItem={(event) => (
              <List.Item
                key={`${event.snapshot.incarnation}-${event.snapshot.generation}-${event.snapshot.snapshot_revision}`}
              >
                <List.Item.Meta
                  title={describeEventCause(event.cause)}
                  description={(
                    <Typography.Text type="secondary">
                      {t('computer.runtime.eventRevision', {
                        revision: event.snapshot.snapshot_revision,
                        receivedAt: new Date(event.received_at).toLocaleString(),
                      })}
                    </Typography.Text>
                  )}
                />
              </List.Item>
            )}
          />
        )}
      </Card>

      <Card title={t('computer.runtime.mcpLifecycle')}>
        <McpConfig
          instanceId={instance.id}
          mode="runtime"
          runtimeDisabled={!actions.can_manage_mcp}
        />
      </Card>
    </Space>
  );
}
