import { Collapse, Descriptions, Empty, List, Space, Tag, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type { ComputerRuntimeSnapshot } from '@/stores/runtimeSnapshot';
import type {
  ComputerRuntimeEventCause,
  ComputerRuntimeEventRecord,
} from '@/stores/runtimeStore';

interface RuntimeDiagnosticsProps {
  runtime: ComputerRuntimeSnapshot;
  recentEvents: ComputerRuntimeEventRecord[];
}

export function RuntimeDiagnostics({ runtime, recentEvents }: RuntimeDiagnosticsProps) {
  const { t } = useTranslation();

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
      case 'client_connection_state_changed':
        return t('computer.runtime.eventCauses.clientConnectionStateChanged', {
          revision: cause.revision,
          status: t(`computer.connection.${cause.status}`),
        });
      case 'client_connection_authority_changed':
        return t('computer.runtime.eventCauses.clientConnectionStateChanged', {
          revision: cause.revision,
          status: t(`computer.connection.${cause.present ? 'connected' : 'disconnected'}`),
        });
      case 'client_diagnostic_changed':
        return t('computer.runtime.eventCauses.clientDiagnosticChanged', {
          operation: cause.operation,
          status: cause.has_error
            ? t('computer.runtime.eventCauses.diagnosticFailed')
            : t('computer.runtime.eventCauses.diagnosticCleared'),
        });
      case 'mcp_diagnostic_changed':
        return t('computer.runtime.eventCauses.mcpDiagnosticChanged', {
          bundleId: cause.bundle_id,
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

  const technicalProblems = (runtime.problems ?? [])
    .filter((problem) => problem.current && problem.technical_detail);

  return (
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
              <Descriptions.Item label={t('computer.runtime.incarnation')}>
                {runtime.incarnation}
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
              {t('computer.runtime.problems.technicalDiagnostics')}
            </Typography.Title>
            {technicalProblems.length === 0 ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t('computer.runtime.problems.noTechnicalDiagnostics')}
              />
            ) : (
              <List
                size="small"
                dataSource={technicalProblems}
                renderItem={(problem) => (
                  <List.Item key={problem.id}>
                    <List.Item.Meta
                      title={`${problem.source} · ${problem.operation}`}
                      description={problem.technical_detail}
                    />
                  </List.Item>
                )}
              />
            )}
            <Typography.Title level={5} style={{ margin: 0 }}>
              {t('computer.runtime.recentEvents')}
            </Typography.Title>
            {recentEvents.length === 0 ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t('computer.runtime.noEvents')}
              />
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
  );
}
