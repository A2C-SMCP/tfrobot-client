import { Alert, Button } from 'antd';
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
    return (
      <Alert
        type="info"
        showIcon
        message={t('desktop.noActiveMcp')}
        description={t('desktop.noActiveMcpDescription')}
        action={onOpenMcp ? (
          <Button size="small" onClick={onOpenMcp}>
            {t('desktop.manageMcp')}
          </Button>
        ) : undefined}
      />
    );
  }

  return null;
}
