import {
  Button,
  Card,
  Col,
  Descriptions,
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
import type { McpServerManagedBy } from '@/stores/mcpStore';
import {
  useRuntimeStore,
  type ComputerRuntimeEventRecord,
} from '@/stores/runtimeStore';
import { RuntimeDiagnostics } from './RuntimeDiagnostics';
import { RuntimeProblems } from './RuntimeProblems';

const EMPTY_RUNTIME_EVENTS: ComputerRuntimeEventRecord[] = [];
type PluginMcpServerOwner = Extract<McpServerManagedBy, { type: 'plugin' }>;

interface ComputerRuntimeProps {
  instance: ComputerInstance;
  loading: boolean;
  canConnect: boolean;
  connectDisabledReason?: string;
  onStartStop: () => void;
  onRestart: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onViewLogs: () => void;
  onOpenPlugin?: (owner: PluginMcpServerOwner) => void;
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
  onViewLogs,
  onOpenPlugin,
}: ComputerRuntimeProps) {
  const { t } = useTranslation();
  const runtime = instance.runtime;
  const actions = runtime.actions;
  const connectionState = instance.connectionState;
  const connectionActions = connectionState?.actions ?? {
    connect: actions.connect,
    disconnect: actions.disconnect,
  };
  const connectionStatus = connectionState?.status
    ?? (instance.clientConnectionPresent ? 'connected' : instance.connectionStatus);
  const showDisconnect = connectionActions.disconnect.enabled
    || connectionStatus === 'disconnecting';
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
  const backendConnectDisabledReason = connectionActions.connect.disabled_reason
    ? t(`computer.connectionActions.disabledReasons.${connectionActions.connect.disabled_reason}`)
    : undefined;
  const disconnectDisabledReason = connectionActions.disconnect.disabled_reason
    ? t(`computer.connectionActions.disabledReasons.${connectionActions.disconnect.disabled_reason}`)
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
  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <RuntimeProblems
        problems={runtime.problems ?? []}
        runtimeActions={actions}
        connectionActions={connectionActions}
        canConnect={canConnect}
        connectDisabledReason={backendConnectDisabledReason ?? connectDisabledReason}
        loading={loading}
        onStartRuntime={onStartStop}
        onRestartRuntime={onRestart}
        onRetryConnection={onConnect}
        onRetryDisconnection={onDisconnect}
        onViewLogs={onViewLogs}
      />

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
            {showDisconnect ? (
              <Tooltip title={disconnectDisabledReason}>
                <Button
                  danger
                  icon={<DisconnectOutlined />}
                  disabled={!connectionActions.disconnect.enabled}
                  loading={connectionStatus === 'disconnecting'}
                  onClick={onDisconnect}
                >
                  {t('connection.disconnect')}
                </Button>
              </Tooltip>
            ) : (
              <Tooltip title={backendConnectDisabledReason ?? connectDisabledReason}>
                <Button
                  icon={<LinkOutlined />}
                  disabled={!canConnect || !connectionActions.connect.enabled}
                  loading={connectionStatus === 'connecting'}
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
            <Tag color={connectionStatus === 'connected' ? 'green' : 'default'}>
              {t(`computer.connection.${connectionStatus}`)}
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
          capability={actions.manage_mcp}
          onStartRuntime={onStartStop}
          onRestartRuntime={onRestart}
          onOpenPlugin={onOpenPlugin}
        />
      </Card>

      <RuntimeDiagnostics runtime={runtime} recentEvents={recentEvents} />
    </Space>
  );
}
