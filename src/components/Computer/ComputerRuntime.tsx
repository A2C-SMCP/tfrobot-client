import {
  Card,
  Col,
  Descriptions,
  Row,
  Space,
  Statistic,
  Tag,
} from 'antd';
import { useTranslation } from 'react-i18next';
import { McpRuntimeControls } from '@/components/McpConfig/McpRuntimeControls';
import type { ComputerInstance } from '@/stores/computerStore';
import type { McpServerManagedBy } from '@/stores/mcpStore';
import {
  connectDisabledReasonTranslationKey,
  connectionOperationTargetLabel,
  type ResolvedComputerConnection,
} from './computerActions';
import { RuntimeProblems } from './RuntimeProblems';

type PluginMcpServerOwner = Extract<McpServerManagedBy, { type: 'plugin' }>;

interface ComputerRuntimeProps {
  instance: ComputerInstance;
  connection: ResolvedComputerConnection;
  loading: boolean;
  onStartStop: () => void;
  onRestart: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onViewLogs: () => void;
  onOpenPlugin?: (owner: PluginMcpServerOwner) => void;
}

export function ComputerRuntime({
  instance,
  connection,
  loading,
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
  const connectDisabledReasonKey = connectDisabledReasonTranslationKey(connection);
  const connectDisabledReason = connectDisabledReasonKey
    ? t(connectDisabledReasonKey)
    : undefined;
  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <RuntimeProblems
        problems={runtime.problems ?? []}
        runtimeActions={actions}
        connectionActions={connection.actions}
        canConnect={connection.canConnect}
        connectDisabledReason={connectDisabledReason}
        loading={loading}
        onStartRuntime={onStartStop}
        onRestartRuntime={onRestart}
        onRetryConnection={onConnect}
        onRetryDisconnection={onDisconnect}
        onViewLogs={onViewLogs}
      />

      <Card title={t('computer.workbench.connection.title')}>
        <Descriptions size="small" column={{ xs: 1, md: 3 }}>
          <Descriptions.Item label={t('computer.workbench.connection.status')}>
            <Tag color={connection.status === 'connected' ? 'green' : 'default'}>
              {t(`computer.connection.${connection.status}`)}
            </Tag>
          </Descriptions.Item>
          {connection.operation && (
            <Descriptions.Item label={t('computer.workbench.connection.operation')}>
              {t(`computer.workbench.connection.operations.${connection.operation}`)}
            </Descriptions.Item>
          )}
          {connection.operation && connection.operationTarget && (
            <Descriptions.Item label={t('computer.workbench.connection.operationTarget')}>
              {connectionOperationTargetLabel(
                connection.operation,
                connection.operationTarget,
              )}
            </Descriptions.Item>
          )}
          {instance.connectionState?.context?.profile_name && (
            <Descriptions.Item label={t('computer.workbench.connection.profile')}>
              {instance.connectionState.context.profile_name}
            </Descriptions.Item>
          )}
        </Descriptions>
      </Card>

      <Card>
        <McpRuntimeControls
          instanceId={instance.id}
          capability={actions.manage_mcp}
          onStartRuntime={onStartStop}
          onRestartRuntime={onRestart}
          onOpenPlugin={onOpenPlugin}
        />
      </Card>

      <Card title={t('computer.workbench.capabilitySummary')}>
        <Row gutter={[16, 16]}>
          <Col xs={12} md={6}>
            <Statistic title={t('dashboard.mcpServers')} value={runtime.mcp_servers} />
          </Col>
          <Col xs={12} md={6}>
            <Statistic
              title={t('computer.runtime.activeMcpServers')}
              value={runtime.active_mcp_servers}
            />
          </Col>
          <Col xs={12} md={6}>
            <Statistic title={t('dashboard.tools')} value={runtime.tools} />
          </Col>
          <Col xs={12} md={6}>
            <Statistic title={t('skills.title')} value={runtime.skills} />
          </Col>
        </Row>
      </Card>
    </Space>
  );
}
