import { Alert, Button, Empty, Space, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type { ComputerRuntimeSnapshot } from '@/stores/runtimeSnapshot';
import { DESKTOP_OPERATIONAL_LIFECYCLES } from './availability';

interface DesktopAvailabilityProps {
  runtime: ComputerRuntimeSnapshot;
  onStartRuntime?: () => void;
  onOpenMcp?: () => void;
}

export function DesktopAvailability({
  runtime,
  onStartRuntime,
  onOpenMcp,
}: DesktopAvailabilityProps) {
  const { t } = useTranslation();

  if (!DESKTOP_OPERATIONAL_LIFECYCLES.has(runtime.lifecycle)) {
    const canStart = runtime.actions.start.enabled;
    return (
      <Alert
        type="warning"
        showIcon
        message={t('desktop.runtimeUnavailable')}
        description={
          canStart
            ? t('desktop.runtimeUnavailableDescription')
            : t('desktop.runtimeTransitionDescription')
        }
        action={canStart && onStartRuntime ? (
          <Button size="small" type="primary" onClick={onStartRuntime}>
            {t('computer.start')}
          </Button>
        ) : undefined}
      />
    );
  }

  if (runtime.active_mcp_servers === 0) {
    // "Nothing to show yet" is an empty state, not a notice: it keeps the drawing light and the
    // next step (start MCP) one click away instead of asking the user to read a banner.
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description={(
          <Space direction="vertical" size={0}>
            <Typography.Text strong>{t('desktop.noActiveMcp')}</Typography.Text>
            <Typography.Text type="secondary">{t('desktop.noActiveMcpDescription')}</Typography.Text>
          </Space>
        )}
      >
        {onOpenMcp && (
          <Button size="small" type="primary" onClick={onOpenMcp}>
            {t('desktop.manageMcp')}
          </Button>
        )}
      </Empty>
    );
  }

  return null;
}
