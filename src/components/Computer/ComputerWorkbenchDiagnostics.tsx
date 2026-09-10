import { PageActivity } from '@/components/Navigation/PageActivity';
import { Collapse } from 'antd';
import type { RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { DebugPanel } from '@/components/DebugPanel';
import { ActivityViewer } from '@/components/ActivityViewer';
import type { ComputerRuntimeSnapshot } from '@/stores/runtimeSnapshot';
import type { ComputerRuntimeEventRecord } from '@/stores/runtimeStore';
import { RuntimeDiagnostics } from './RuntimeDiagnostics';
import styles from './ComputerWorkbench.module.css';

interface ComputerWorkbenchDiagnosticsProps {
  instanceId: string;
  runtime: ComputerRuntimeSnapshot;
  recentEvents: ComputerRuntimeEventRecord[];
  activePanels: string[];
  debugRef: RefObject<HTMLDivElement>;
  logsRef: RefObject<HTMLDivElement>;
  onPanelsChange: (keys: string | string[]) => void;
}

export function ComputerWorkbenchDiagnostics({
  instanceId,
  runtime,
  recentEvents,
  activePanels,
  debugRef,
  logsRef,
  onPanelsChange,
}: ComputerWorkbenchDiagnosticsProps) {
  const { t } = useTranslation();

  return (
    <section
      className={`${styles.section} ${styles.diagnostics}`}
      aria-label={t('computer.workbench.sections.diagnostics')}
    >
      <Collapse
        activeKey={activePanels}
        onChange={onPanelsChange}
        items={[
          {
            key: 'debug',
            label: t('nav.debugPanel'),
            children: (
              <div ref={debugRef} tabIndex={-1}>
                <PageActivity active={activePanels.includes('debug')}><DebugPanel instanceId={instanceId} /></PageActivity>
              </div>
            ),
          },
          {
            key: 'logs',
            label: t('logs.title'),
            children: (
              <div ref={logsRef} tabIndex={-1}>
                <PageActivity active={activePanels.includes('logs')}><ActivityViewer instanceId={instanceId} /></PageActivity>
              </div>
            ),
          },
        ]}
      />
      <RuntimeDiagnostics runtime={runtime} recentEvents={recentEvents} />
    </section>
  );
}
