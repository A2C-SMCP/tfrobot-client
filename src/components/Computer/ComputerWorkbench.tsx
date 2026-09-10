import { Card } from 'antd';
import { useTranslation } from 'react-i18next';
import { DesktopResources } from '@/components/DesktopResources';
import type { ComputerInstance } from '@/stores/computerStore';
import { useManagerStore } from '@/stores/managerStore';
import {
  useRuntimeStore,
  type ComputerRuntimeEventRecord,
} from '@/stores/runtimeStore';
import type { McpServerManagedBy } from '@/stores/mcpStore';
import { ComputerRuntime } from './ComputerRuntime';
import { ComputerWorkbenchDiagnostics } from './ComputerWorkbenchDiagnostics';
import { ComputerWorkbenchHeader } from './ComputerWorkbenchHeader';
import { SkillsTab } from './SkillsTab';
import { resolveComputerConnection } from './computerActions';
import type { ComputerWorkbenchSection } from './tabs';
import { useComputerWorkbenchSections } from './useComputerWorkbenchSections';
import styles from './ComputerWorkbench.module.css';

const EMPTY_RUNTIME_EVENTS: ComputerRuntimeEventRecord[] = [];
type PluginMcpServerOwner = Extract<McpServerManagedBy, { type: 'plugin' }>;

interface ComputerWorkbenchProps {
  instance: ComputerInstance;
  loading: boolean;
  initialSection: ComputerWorkbenchSection;
  navigationRevision?: number;
  onBack: () => void;
  onOpenSettings: () => void;
  onDelete: () => Promise<void>;
  onStartStop: () => void;
  onRestart: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onOpenPlugin?: (owner: PluginMcpServerOwner) => void;
}

export function ComputerWorkbench({
  instance,
  loading,
  initialSection,
  navigationRevision,
  onBack,
  onOpenSettings,
  onDelete,
  onStartStop,
  onRestart,
  onConnect,
  onDisconnect,
  onOpenPlugin,
}: ComputerWorkbenchProps) {
  const { t } = useTranslation();
  const currentManagerContext = useManagerStore((state) => state.context.contextKey);
  const connection = resolveComputerConnection(instance, currentManagerContext);
  const recentEvents = useRuntimeStore((state) => state.eventsByInstance[instance.id])
    ?? EMPTY_RUNTIME_EVENTS;
  const {
    sectionRefs,
    diagnosticPanels,
    changeDiagnosticPanels,
    openSection,
  } = useComputerWorkbenchSections(initialSection, navigationRevision);

  return (
    <div className={styles.page} aria-label={t('computer.workbench.pageLabel')}>
      <ComputerWorkbenchHeader
        instance={instance}
        connection={connection}
        loading={loading}
        onBack={onBack}
        onOpenSettings={onOpenSettings}
        onDelete={onDelete}
        onStartStop={onStartStop}
        onRestart={onRestart}
        onConnect={onConnect}
        onDisconnect={onDisconnect}
      />

      <main>
        <section
          ref={sectionRefs.top}
          className={styles.section}
          tabIndex={-1}
          aria-label={t('computer.workbench.sections.runtime')}
        >
          <ComputerRuntime
            instance={instance}
            connection={connection}
            loading={loading}
            onStartStop={onStartStop}
            onRestart={onRestart}
            onConnect={onConnect}
            onDisconnect={onDisconnect}
            onViewLogs={() => openSection('logs')}
            onOpenPlugin={onOpenPlugin}
          />
        </section>

        <section
          ref={sectionRefs.skills}
          className={styles.section}
          tabIndex={-1}
          aria-label={t('computer.workbench.sections.skills')}
        >
          <Card className={styles.sectionCard}>
            <SkillsTab instanceId={instance.id} />
          </Card>
        </section>

        <section
          ref={sectionRefs.resources}
          className={styles.section}
          tabIndex={-1}
          aria-label={t('computer.workbench.sections.resources')}
        >
          <DesktopResources
            instanceId={instance.id}
            runtime={instance.runtime}
            initiallyExpanded={initialSection === 'resources'}
            onStartRuntime={onStartStop}
            onOpenMcp={() => openSection('top')}
          />
        </section>

        <ComputerWorkbenchDiagnostics
          instanceId={instance.id}
          runtime={instance.runtime}
          recentEvents={recentEvents}
          activePanels={diagnosticPanels}
          debugRef={sectionRefs.debug}
          logsRef={sectionRefs.logs}
          onPanelsChange={changeDiagnosticPanels}
        />
      </main>
    </div>
  );
}
