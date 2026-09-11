import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { useEffect } from 'react';
import {
  Alert,
  Button,
  Collapse,
  Space,
} from 'antd';
import { DesktopOutlined, ReloadOutlined } from '@ant-design/icons';
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
  initiallyExpanded?: boolean;
  onStartRuntime?: () => void;
  onOpenMcp?: () => void;
}

export function DesktopResources({
  instanceId,
  runtime,
  initiallyExpanded = false,
  onStartRuntime,
  onOpenMcp,
}: DesktopResourcesProps) {
  const { t } = useTranslation();
  const runtimeKey = desktopRuntimeKey(runtime);
  const desktop = useDesktopStore(
    (state) => selectDesktopInstance(state, instanceId, runtimeKey),
  );
  const bindRuntime = useDesktopStore((state) => state.bindRuntime);
  const fetchDesktop = useDesktopStore((state) => state.fetchDesktop);
  const fetchWindowDetail = useDesktopStore((state) => state.fetchWindowDetail);
  const [activePanels, setActivePanels] = useNavigationState<string[]>('desktop.panels',
    initiallyExpanded ? ['desktop-resources'] : [],
  );
  const canLoad = canEnumerateDesktopResources(runtime);

  useEffect(() => {
    bindRuntime(instanceId, runtimeKey);
  }, [bindRuntime, instanceId, runtimeKey]);

  useEffect(() => {
    if (initiallyExpanded) {
      setActivePanels(['desktop-resources']);
    }
  }, [initiallyExpanded, setActivePanels]);

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
          <Space>
            <Button
              type={desktop.loaded ? 'default' : 'primary'}
              icon={desktop.loaded ? <ReloadOutlined /> : <DesktopOutlined />}
              aria-label={desktop.loaded ? t('common.refresh') : t('desktop.loadResources')}
              onClick={() => { void fetchDesktop(instanceId, runtimeKey); }}
              loading={desktop.loading}
            >
              {desktop.loaded ? t('common.refresh') : t('desktop.loadResources')}
            </Button>
          </Space>

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
    <Collapse
      activeKey={activePanels}
      onChange={(keys) => setActivePanels(Array.isArray(keys) ? keys : [keys])}
      items={[
        {
          key: 'desktop-resources',
          label: t('desktop.title'),
          children: content,
        },
      ]}
    />
  );
}
