import { Card, Tabs } from 'antd';
import { useTranslation } from 'react-i18next';
import { PageHost } from '@/components/Navigation/PageHost';
import { DebugPanel } from '@/components/DebugPanel';
import { ActivityViewer } from '@/components/ActivityViewer';
import type { DiagnosticTab } from './useComputerWorkbenchSections';
import styles from './ComputerWorkbench.module.css';

interface Props {
  instanceId: string;
  activeTab: DiagnosticTab;
  onTabChange: (tab: DiagnosticTab) => void;
}
export function ComputerWorkbenchDiagnostics({ instanceId, activeTab, onTabChange }: Props) {
  const { t } = useTranslation();
  return <section aria-label={t('computer.redesign.diagnostics')}>
    <Tabs activeKey={activeTab} onChange={(key) => onTabChange(key as DiagnosticTab)} items={[
      { key: 'logs', label: t('logs.title'), children: <PageHost name="diagnostics-logs" active={activeTab === 'logs'}><Card className={styles.diagnosticCard}><ActivityViewer instanceId={instanceId} /></Card></PageHost> },
      { key: 'debug', label: t('nav.debugPanel'), children: <PageHost name="diagnostics-debug" active={activeTab === 'debug'}><Card className={styles.diagnosticCard}><DebugPanel instanceId={instanceId} /></Card></PageHost> },
    ]} />
  </section>;
}
