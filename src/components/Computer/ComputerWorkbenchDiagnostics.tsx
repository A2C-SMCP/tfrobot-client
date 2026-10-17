import { Card } from 'antd';
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
  debugRef: RefObject<HTMLDivElement>;
  logsRef: RefObject<HTMLDivElement>;
}

export function ComputerWorkbenchDiagnostics({
  instanceId,
  runtime,
  recentEvents,
  debugRef,
  logsRef,
}: ComputerWorkbenchDiagnosticsProps) {
  const { t } = useTranslation();

  return (
    <section
      className={`${styles.section} ${styles.diagnostics}`}
      aria-label={t('computer.workbench.sections.diagnostics')}
    >
      <section ref={debugRef} tabIndex={-1} className={styles.section} aria-label={t('nav.debugPanel')}>
        <Card title={t('nav.debugPanel')} className={`${styles.sectionCard} ${styles.diagnosticCard}`}>
          <DebugPanel instanceId={instanceId} />
        </Card>
      </section>
      <section ref={logsRef} tabIndex={-1} className={styles.section} aria-label={t('logs.title')}>
        <Card className={`${styles.sectionCard} ${styles.diagnosticCard}`}>
          <ActivityViewer instanceId={instanceId} />
        </Card>
      </section>
      <RuntimeDiagnostics runtime={runtime} recentEvents={recentEvents} />
    </section>
  );
}
