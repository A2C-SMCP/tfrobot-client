import type { ReactNode } from 'react';
import { Card, Descriptions, Tag, Typography } from 'antd';
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
}: ComputerRuntimeProps) {
  const { t } = useTranslation();
  const runtime = instance.runtime;
  const actions = runtime.actions;
  const currentContext = instance.connectionState?.context ?? instance.clientConnectionContext;
  const present = instance.connectionState?.present ?? instance.clientConnectionPresent ?? connection.status === 'connected';
  const currentTarget = present ? currentContext?.target_name || currentContext?.profile_name : undefined;
  const autoConnect = instance.connectionPolicy.auto_connect;
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
      <Card className={styles.connectionSummary} title={t('computer.redesign.connectionOverview')}>
        <Descriptions column={{ xs: 1, sm: 2, lg: 3 }} size="small" layout="vertical" colon={false}>
          <Descriptions.Item label={t('computer.redesign.currentConnection')}>
            <Tag color={connection.status === 'connected' ? 'success' : 'default'}>
              {t(`computer.connection.${connection.status}`)}
            </Tag>
            {currentTarget && <Typography.Text strong>{currentTarget}</Typography.Text>}
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.redesign.nextConnection')}>
            <Typography.Text strong>{targetName}</Typography.Text>
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.redesign.autoConnectAtStartup')}>
            <Tag color={autoConnect ? 'success' : 'default'}>
              {t(autoConnect ? 'computer.redesign.autoConnectOn' : 'computer.redesign.autoConnectOff')}
            </Tag>
          </Descriptions.Item>
        </Descriptions>
      </Card>

      <Card className={styles.runtimeMcp}>
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
