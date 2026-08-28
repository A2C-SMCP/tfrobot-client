import { Alert, Button, Space, Tag, Typography } from 'antd';
import dayjs from 'dayjs';
import { useTranslation } from 'react-i18next';
import type {
  ComputerRuntimeActionCapabilities,
  ComputerRuntimeProblem,
  ComputerRuntimeProblemAction,
} from '@/stores/runtimeSnapshot';
import { affectedCapabilityLabel } from './runtimeProblemPresentation';

interface ProblemConnectionActions {
  connect: { enabled: boolean; disabled_reason?: string | null };
  disconnect: { enabled: boolean; disabled_reason?: string | null };
}

interface RuntimeProblemsProps {
  problems: ComputerRuntimeProblem[];
  runtimeActions: ComputerRuntimeActionCapabilities;
  connectionActions: ProblemConnectionActions;
  canConnect: boolean;
  connectDisabledReason?: string;
  loading: boolean;
  onStartRuntime: () => void;
  onRestartRuntime: () => void;
  onRetryConnection: () => void;
  onRetryDisconnection: () => void;
  onViewLogs: () => void;
}

interface ResolvedProblemAction {
  enabled: boolean;
  label: string;
  disabledReason?: string;
  run: () => void;
}

export function RuntimeProblems({
  problems,
  runtimeActions,
  connectionActions,
  canConnect,
  connectDisabledReason,
  loading,
  onStartRuntime,
  onRestartRuntime,
  onRetryConnection,
  onRetryDisconnection,
  onViewLogs,
}: RuntimeProblemsProps) {
  const { t } = useTranslation();
  const currentProblems = problems.filter((problem) => problem.current);

  const resolveAction = (action: ComputerRuntimeProblemAction): ResolvedProblemAction => {
    switch (action) {
      case 'start_runtime':
        return {
          enabled: runtimeActions.start.enabled,
          label: t('computer.start'),
          disabledReason: runtimeActions.start.disabled_reason
            ? t(`computer.runtime.actionDisabledReasons.${runtimeActions.start.disabled_reason}`)
            : undefined,
          run: onStartRuntime,
        };
      case 'restart_runtime':
        return {
          enabled: runtimeActions.restart.enabled,
          label: t('computer.runtime.restart'),
          disabledReason: runtimeActions.restart.disabled_reason
            ? t(`computer.runtime.actionDisabledReasons.${runtimeActions.restart.disabled_reason}`)
            : undefined,
          run: onRestartRuntime,
        };
      case 'retry_connection':
        return {
          enabled: canConnect && connectionActions.connect.enabled,
          label: t('computer.runtime.problems.actions.retryConnection'),
          disabledReason: connectionActions.connect.disabled_reason
            ? t(`computer.connectionActions.disabledReasons.${connectionActions.connect.disabled_reason}`)
            : connectDisabledReason,
          run: onRetryConnection,
        };
      case 'retry_disconnection':
        return {
          enabled: connectionActions.disconnect.enabled,
          label: t('computer.runtime.problems.actions.retryDisconnection'),
          disabledReason: connectionActions.disconnect.disabled_reason
            ? t(`computer.connectionActions.disabledReasons.${connectionActions.disconnect.disabled_reason}`)
            : undefined,
          run: onRetryDisconnection,
        };
      case 'view_logs':
        return {
          enabled: true,
          label: t('computer.runtime.problems.actions.viewLogs'),
          run: onViewLogs,
        };
    }
  };

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      {currentProblems.map((problem) => {
        const resolvedActions = problem.recommended_actions.map(resolveAction);
        const disabledActions = resolvedActions.filter((action) => !action.enabled);
        const presentationDetail = problem.message === 'mcp_start_failed'
          ? problem.presentation_detail
          : undefined;
        return (
          <Alert
            key={problem.id}
            type={problem.severity === 'error' ? 'error' : 'warning'}
            showIcon
            message={t(`computer.runtime.problems.messages.${problem.message}`)}
            description={(
              <Space direction="vertical" size={8} style={{ width: '100%' }}>
                <Space wrap size={[4, 4]}>
                  <Tag color={problem.severity === 'error' ? 'red' : 'orange'}>
                    {t(`computer.runtime.problems.severity.${problem.severity}`)}
                  </Tag>
                  <Tag>{t(`computer.runtime.problems.sources.${problem.source}`)}</Tag>
                  <Typography.Text>
                    {t('computer.runtime.problems.affected', {
                      capabilities: problem.affected_capabilities
                        .map((capability) => affectedCapabilityLabel(capability, t))
                        .join(', '),
                    })}
                  </Typography.Text>
                </Space>
                <Typography.Text type="secondary">
                  {t('computer.runtime.problems.occurredAt', {
                    time: dayjs(problem.occurred_at).format('YYYY-MM-DD HH:mm:ss'),
                  })}
                </Typography.Text>
                {presentationDetail && (
                  <Typography.Paragraph
                    type="secondary"
                    style={{ margin: 0, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}
                  >
                    {t('computer.runtime.problems.technicalDetail', {
                      detail: presentationDetail,
                    })}
                  </Typography.Paragraph>
                )}
                <Space wrap>
                  {resolvedActions.map((action) => (
                    <Button
                      key={action.label}
                      size="small"
                      disabled={!action.enabled}
                      loading={loading && action.enabled}
                      onClick={action.run}
                    >
                      {action.label}
                    </Button>
                  ))}
                </Space>
                {disabledActions.map((action) => (
                  <Typography.Text key={action.label} type="secondary">
                    {t('computer.runtime.problems.actionUnavailable', {
                      action: action.label,
                      reason: action.disabledReason
                        ?? t('computer.runtime.problems.actions.unavailable'),
                    })}
                  </Typography.Text>
                ))}
              </Space>
            )}
          />
        );
      })}
    </Space>
  );
}
