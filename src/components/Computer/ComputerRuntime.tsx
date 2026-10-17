import type { ReactNode } from 'react';
import {
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
}

export function ComputerRuntime({
  children,
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
}: ComputerRuntimeProps) {
  const { t } = useTranslation();
  const runtime = instance.runtime;
  const actions = runtime.actions;
  const connectDisabledReasonKey = connectDisabledReasonTranslationKey(connection);
  const connectDisabledReason = connectDisabledReasonKey
    ? t(connectDisabledReasonKey)
    : undefined;
  return (
    <div className={styles.runtimeGrid}>
      <div className={styles.runtimeProblems}>
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
      </div>
      <Card className={styles.capabilitySummary} title={t('computer.workbench.capabilitySummary')}>
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

      <Card className={styles.runtimeMcp}>
        <McpRuntimeControls
          instanceId={instance.id}
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
