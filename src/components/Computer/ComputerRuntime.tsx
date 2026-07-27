import {
  Alert,
  Button,
  Card,
  Col,
  Collapse,
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
  RetweetOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { McpRuntimeControls } from '@/components/McpConfig/McpRuntimeControls';
import type { ComputerInstance } from '@/stores/computerStore';
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
  onConnect,
  onDisconnect,
}: ComputerRuntimeProps) {
  const { t } = useTranslation();
  const runtime = instance.runtime;
  const actions = runtime.actions;
  const hasClientConnection = instance.clientConnectionPresent
    ?? instance.connectionStatus === 'connected';
  const recentEvents = useRuntimeStore((state) => state.eventsByInstance[instance.id])
    ?? EMPTY_RUNTIME_EVENTS;
  const primaryAction = ['running', 'degraded', 'stopping'].includes(runtime.user_state)
    ? 'stop'
    : 'start';
  const primaryCapability = actions[primaryAction];
  const primaryActionLabel = runtime.user_state === 'starting'
    ? t('computer.runtime.actionProgress.starting')
    : runtime.user_state === 'stopping'
      ? t('computer.runtime.actionProgress.stopping')
      : primaryAction === 'stop'
        ? t('computer.stop')
        : t('computer.start');
  const primaryDisabledReason = primaryCapability.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${primaryCapability.disabled_reason}`)
    : undefined;
  const restartDisabledReason = actions.restart.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${actions.restart.disabled_reason}`)
    : undefined;
  const backendConnectDisabledReason = actions.connect.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${actions.connect.disabled_reason}`)
    : undefined;
  const disconnectDisabledReason = actions.disconnect.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${actions.disconnect.disabled_reason}`)
    : undefined;
  const mcpDisabledReason = actions.manage_mcp.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${actions.manage_mcp.disabled_reason}`)
    : undefined;
  const runtimeStateColor = runtime.user_state === 'error'
    ? 'red'
    : runtime.user_state === 'degraded'
      ? 'orange'
      : runtime.user_state === 'running'
        ? 'green'
        : runtime.user_state === 'starting' || runtime.user_state === 'stopping'
          ? 'blue'
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
              type="primary"
              icon={primaryAction === 'stop' ? <StopOutlined /> : <PlayCircleOutlined />}
              disabled={!primaryCapability.enabled}
              loading={loading}
              onClick={onStartStop}
            >
              {primaryActionLabel}
            </Button>
            <Tooltip title={restartDisabledReason}>
              <Button
                icon={<RetweetOutlined />}
                disabled={!actions.restart.enabled}
                loading={loading}
                onClick={onRestart}
              >
                {t('computer.runtime.restart')}
              </Button>
            </Tooltip>
            {hasClientConnection ? (
              <Tooltip title={disconnectDisabledReason}>
                <Button
                  danger
                  icon={<DisconnectOutlined />}
                  disabled={!actions.disconnect.enabled}
                  loading={loading}
                  onClick={onDisconnect}
                >
                  {t('connection.disconnect')}
                </Button>
              </Tooltip>
            ) : (
              <Tooltip title={backendConnectDisabledReason ?? connectDisabledReason}>
                <Button
                  icon={<LinkOutlined />}
                  disabled={!canConnect || !actions.connect.enabled}
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
        <Space direction="vertical" size={8} style={{ width: '100%' }}>
          <Descriptions column={{ xs: 1, sm: 2 }} size="small">
            <Descriptions.Item label={t('computer.runtime.userState')}>
              <Tag color={runtimeStateColor}>
                {t(`computer.runtime.userStates.${runtime.user_state}`)}
              </Tag>
            </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.connection')}>
            <Tag color={instance.connectionStatus === 'connected' ? 'green' : 'default'}>
              {t(`computer.connection.${instance.connectionStatus}`)}
            </Tag>
          </Descriptions.Item>
          </Descriptions>
          {primaryDisabledReason && (
            <Typography.Text type="secondary">{primaryDisabledReason}</Typography.Text>
          )}
        </Space>
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

      <Card title={t('computer.runtime.mcpLifecycle')}>
        <McpRuntimeControls
          instanceId={instance.id}
          disabled={!actions.manage_mcp.enabled}
        />
        {mcpDisabledReason && (
          <Typography.Text type="secondary">{mcpDisabledReason}</Typography.Text>
        )}
      </Card>

      <Collapse
        items={[{
          key: 'advanced-runtime-diagnostics',
          label: t('computer.runtime.advancedDiagnostics'),
          children: (
            <Space direction="vertical" size={16} style={{ width: '100%' }}>
              <Descriptions column={{ xs: 1, sm: 2, lg: 3 }} size="small">
                <Descriptions.Item label={t('computer.runtime.lifecycle')}>
                  <Tag>{t(`computer.runtime.lifecycleStates.${runtime.lifecycle}`)}</Tag>
                </Descriptions.Item>
                <Descriptions.Item label={t('computer.runtime.generation')}>
                  {runtime.generation}
                </Descriptions.Item>
                <Descriptions.Item label={t('computer.runtime.snapshotRevision')}>
                  {runtime.snapshot_revision}
                </Descriptions.Item>
                <Descriptions.Item label={t('computer.runtime.configRevision')}>
                  {runtime.config_revision}
                </Descriptions.Item>
                <Descriptions.Item label={t('computer.runtime.capabilityRevision')}>
                  {runtime.capability_revision}
                </Descriptions.Item>
              </Descriptions>
              <Typography.Title level={5} style={{ margin: 0 }}>
                {t('computer.runtime.recentEvents')}
              </Typography.Title>
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
            </Space>
          ),
        }]}
      />
    </Space>
  );
}
