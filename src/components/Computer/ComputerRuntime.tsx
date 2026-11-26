import type { ReactNode } from 'react';
import {
  Button,
  Descriptions,
  Space,
  Typography,
  Card,
  Col,
  Row,
  Statistic,
} from 'antd';
import { useTranslation } from 'react-i18next';
import { McpRuntimeControls } from '@/components/McpConfig/McpRuntimeControls';
import type { ComputerInstance } from '@/stores/computerStore';
import type { McpServerManagedBy } from '@/stores/mcpStore';
import {
  connectDisabledReasonTranslationKey,
  type ResolvedComputerConnection,
} from './computerActions';
import { RuntimeProblems } from './RuntimeProblems';
import styles from './ComputerWorkbench.module.css';

type PluginMcpServerOwner = Extract<McpServerManagedBy, { type: 'plugin' }>;

interface ComputerRuntimeProps {
  children?: ReactNode;
  focusedMcp?: string | null;
  focusRequest?: number;
  instance: ComputerInstance;
  connection: ResolvedComputerConnection;
  loading: boolean;
  onStartStop: () => void;
  onRestart: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onViewLogs: () => void;
  onOpenPlugin?: (owner: PluginMcpServerOwner) => void;
  /** Opens Settings → Permissions & security from the keychain notice. */
  onOpenPermissionHelp?: () => void;
  onOpenConnectionSettings?: () => void;
  onOpenMcpSettings?: () => void;
}

export function ComputerRuntime({
  children,
  focusedMcp,
  focusRequest,
  instance,
  connection,
  loading,
  onStartStop,
  onRestart,
  onConnect,
  onDisconnect,
  onViewLogs,
  onOpenPlugin,
  onOpenPermissionHelp,
  onOpenConnectionSettings,
  onOpenMcpSettings,
}: ComputerRuntimeProps) {
  const { t } = useTranslation();
  const runtime = instance.runtime;
  const actions = runtime.actions;
  const observed = !['created', 'starting', 'stopping', 'stopped', 'shutdown', 'error'].includes(runtime.lifecycle);
  const currentContext = instance.connectionState?.context ?? instance.clientConnectionContext;
  const present = instance.connectionState?.present ?? instance.clientConnectionPresent ?? connection.status === 'connected';
  const target = instance.connectionPolicy.target;
  const targetName = target?.type === 'manager_robot' ? instance.robotName ?? t('computer.connectionActions.savedManagerRobot', { employeeId: target.employeeId })
    : target?.type === 'manual_smcp' ? t('computer.redesign.manualTarget', { id: target.id }) : t('computer.redesign.noTarget');
  const connectDisabledReasonKey = connectDisabledReasonTranslationKey(connection);
  const connectDisabledReason = connectDisabledReasonKey
    ? t(connectDisabledReasonKey)
    : undefined;
  return (
    <div className={styles.runtimeGrid}>
      {runtime.problems?.some((problem) => problem.current) && <div className={styles.runtimeProblems}>
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
      </div>}
      <Card className={styles.connectionSummary} title={t('computer.redesign.connectionOverview')}
        extra={onOpenConnectionSettings && <Button type="link" onClick={onOpenConnectionSettings}>{t('computer.redesign.editConnection')}</Button>}>
        <Descriptions column={{ xs: 1, sm: 2 }} size="small">
          <Descriptions.Item label={t('computer.redesign.currentConnection')}>
            {present ? currentContext?.target_name || currentContext?.profile_name || t('computer.connection.connected') : t(`computer.connection.${connection.status}`)}
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.redesign.nextConnection')}>{targetName}</Descriptions.Item>
        </Descriptions>
        <Typography.Text type="secondary">{t(instance.connectionPolicy.auto_connect ? 'computer.redesign.autoConnectOn' : 'computer.redesign.autoConnectOff')}</Typography.Text>
      </Card>
      <Card className={styles.capabilitySummary} title={t('computer.redesign.observedCapabilities')}>
        <Row gutter={[16, 16]}>
          <Col xs={12} md={8}><Statistic title={t('computer.runtime.activeMcpServers')} value={observed ? runtime.active_mcp_servers : '—'} /></Col>
          <Col xs={12} md={8}><Statistic title={t('computer.redesign.availableTools')} value={observed ? runtime.tools : '—'} /></Col>
          <Col xs={12} md={8}><Statistic title={t('computer.redesign.availableSkills')} value={observed ? runtime.skills : '—'} /></Col>
        </Row>
        {!observed && <Typography.Text type="secondary">{t('computer.redesign.observationWaiting')}</Typography.Text>}
      </Card>

      <Card className={styles.runtimeMcp}>
        {onOpenMcpSettings && <Space className={styles.moduleToolbar}><Typography.Text type="secondary">{t('computer.redesign.runtimeMcpDescription')}</Typography.Text><Button onClick={onOpenMcpSettings}>{t('computer.redesign.editMcp')}</Button></Space>}
        <McpRuntimeControls
          instanceId={instance.id}
          showRuntimeGuidance={false}
          focusedBundleId={focusedMcp}
          focusRequest={focusRequest}
          capability={actions.manage_mcp}
          onStartRuntime={onStartStop}
          onRestartRuntime={onRestart}
          onOpenPlugin={onOpenPlugin}
          onOpenPermissionHelp={onOpenPermissionHelp}
        />
      </Card>
      {children}
    </div>
  );
}
