import { useEffect } from 'react';
import { usePageActive } from '@/components/Navigation/pageActivityState';
import {
  Alert,
  Button,
  Card,
  Space,
} from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  desktopRuntimeKey,
  selectDesktopInstance,
  useDesktopStore,
} from '@/stores/desktopStore';
import type { ComputerRuntimeSnapshot } from '@/stores/runtimeSnapshot';
import { canEnumerateDesktopResources } from './availability';
import { DesktopAvailability } from './DesktopAvailability';
import { DesktopResourcesTable } from './DesktopResourcesTable';

interface DesktopResourcesProps {
  instanceId: string;
  runtime: ComputerRuntimeSnapshot;
  onStartRuntime?: () => void;
  onOpenMcp?: () => void;
}

export function DesktopResources({
  instanceId,
  runtime,
  onStartRuntime,
  onOpenMcp,
}: DesktopResourcesProps) {
  const { t } = useTranslation();
  const pageActive = usePageActive();
  const runtimeKey = desktopRuntimeKey(runtime);
  const desktop = useDesktopStore(
    (state) => selectDesktopInstance(state, instanceId, runtimeKey),
  );
  const bindRuntime = useDesktopStore((state) => state.bindRuntime);
  const fetchDesktop = useDesktopStore((state) => state.fetchDesktop);
  const fetchWindowDetail = useDesktopStore((state) => state.fetchWindowDetail);
  const canLoad = canEnumerateDesktopResources(runtime);

  useEffect(() => {
    bindRuntime(instanceId, runtimeKey);
  }, [bindRuntime, instanceId, runtimeKey]);

  useEffect(() => {
    if (!pageActive || !canLoad) return;
    const current = selectDesktopInstance(useDesktopStore.getState(), instanceId, runtimeKey);
    if (!current.loaded && !current.loading && !current.error) {
      void fetchDesktop(instanceId, runtimeKey);
    }
  }, [pageActive, canLoad, fetchDesktop, instanceId, runtimeKey]);

  const content = (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {runtime.lifecycle === 'degraded' && (
        <Alert
          type="warning"
          showIcon
          message={t('desktop.runtimeDegraded')}
          description={t('desktop.runtimeDegradedDescription')}
        />
      )}

      {!canLoad ? (
        <DesktopAvailability
          runtime={runtime}
          onStartRuntime={onStartRuntime}
          onOpenMcp={onOpenMcp}
        />
      ) : (
        <>
          {desktop.error && (
            <Alert
              message={t('desktop.enumerationFailed')}
              description={desktop.error}
              type="error"
              showIcon
              action={(
                <Button
                  size="small"
                  onClick={() => { void fetchDesktop(instanceId, runtimeKey); }}
                >
                  {t('common.retry')}
                </Button>
              )}
            />
          )}

          <DesktopResourcesTable
            desktop={desktop}
            instanceId={instanceId}
            runtimeKey={runtimeKey}
            fetchWindowDetail={fetchWindowDetail}
          />
        </>
      )}
    </Space>
  );

  return (
    <Card
      title={t('desktop.title')}
      extra={(
        <Button
          icon={<ReloadOutlined />}
          aria-label={t('common.refresh')}
          disabled={!canLoad}
          onClick={() => { void fetchDesktop(instanceId, runtimeKey); }}
          loading={desktop.loading}
        >
          {t('common.refresh')}
        </Button>
      )}
    >
      {content}
    </Card>
  );
}
