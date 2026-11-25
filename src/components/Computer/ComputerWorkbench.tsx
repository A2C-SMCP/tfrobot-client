import { useEffect } from 'react';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { Button, Card, Empty, Tabs } from 'antd';
import { useTranslation } from 'react-i18next';
import { ConfigAdoptionStatus } from './ConfigAdoptionStatus';
import { PageHost } from '@/components/Navigation/PageHost';
import { DesktopResources } from '@/components/DesktopResources';
import { isRuntimeRunning, type ComputerInstance } from '@/stores/computerStore';
import { useManagerStore } from '@/stores/managerStore';
import type { McpServerManagedBy } from '@/stores/mcpStore';
import { ComputerRuntime } from './ComputerRuntime';
import { ComputerWorkbenchDiagnostics } from './ComputerWorkbenchDiagnostics';
import { ComputerWorkbenchHeader } from './ComputerWorkbenchHeader';
import { SkillsTab } from './SkillsTab';
import { resolveComputerConnection } from './computerActions';
import type { ComputerWorkbenchSection } from './tabs';
import { useComputerWorkbenchSections } from './useComputerWorkbenchSections';
import styles from './ComputerWorkbench.module.css';

type PluginMcpServerOwner = Extract<McpServerManagedBy, { type: 'plugin' }>;
interface ComputerWorkbenchProps {
  instance: ComputerInstance;
  targetMcpId?: string | null;
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
  onOpenPermissionHelp?: () => void;
  onNavigate?: (key: string) => void;
}

export function ComputerWorkbench({ instance, targetMcpId, loading, initialSection, navigationRevision, onBack, onOpenSettings, onDelete, onStartStop, onRestart, onConnect, onDisconnect, onOpenPlugin, onOpenPermissionHelp, onNavigate }: ComputerWorkbenchProps) {
  const { t } = useTranslation();
  const managerContext = useManagerStore((state) => state.context.contextKey);
  const connection = resolveComputerConnection(instance, managerContext);
  const { activeSection, diagnosticTab, setDiagnosticTab, openSection } = useComputerWorkbenchSections(initialSection, navigationRevision);
  const [focusedMcp, setFocusedMcp] = useNavigationState<string | null>('workbench.focusMcp', targetMcpId ?? null);
  const [focusRequest, setFocusRequest] = useNavigationState('workbench.focusRequest', navigationRevision ?? 0);
  useEffect(() => { if (targetMcpId) { setFocusedMcp(targetMcpId); setFocusRequest(navigationRevision ?? 0); } }, [targetMcpId, navigationRevision, setFocusedMcp, setFocusRequest]);
  const running = isRuntimeRunning(instance.runtime);
  return <div className={styles.page} aria-label={t('computer.workbench.pageLabel')}>
    <ComputerWorkbenchHeader instance={instance} connection={connection} loading={loading} onBack={onBack} onOpenSettings={onOpenSettings}
      onDelete={onDelete} onStartStop={onStartStop} onRestart={onRestart} onConnect={onConnect} onDisconnect={onDisconnect} />
    <ConfigAdoptionStatus quiet key={instance.id} instance={instance} onRestart={onRestart} onOpenPlugin={onOpenPlugin} />
    <main>
      <Tabs className={styles.taskTabs} activeKey={activeSection} onChange={(section) => openSection(section as ComputerWorkbenchSection)} items={[
        { key: 'top', label: t('computer.redesign.overview'), children: <PageHost name="workbench-top" active={activeSection === 'top'}>
        <ComputerRuntime focusRequest={focusRequest} focusedMcp={focusedMcp} instance={instance} connection={connection} loading={loading}
          onStartStop={onStartStop} onRestart={onRestart} onConnect={onConnect} onDisconnect={onDisconnect}
          onViewLogs={() => openSection('logs')} onOpenBuiltIn={() => onNavigate?.('computer-settings:built-in-tools')} onOpenPlugin={onOpenPlugin} onOpenPermissionHelp={onOpenPermissionHelp}
          onOpenConnectionSettings={() => onNavigate?.('computer-settings:connection')}
          onOpenMcpSettings={() => onNavigate?.('computer-settings:mcp')} />
      </PageHost> },
        { key: 'skills', label: t('computer.redesign.skillsTools'), children: <PageHost name="workbench-skills" active={activeSection === 'skills'}>
        <Card className={styles.sectionCard}>
          <div className={styles.moduleToolbar}>
            <span>{t('computer.redesign.activeCapabilities')}</span>
            <Button onClick={() => onNavigate?.('computer-settings:plugins')}>{t('computer.redesign.manageCapabilities')}</Button>
          </div>
          <SkillsTab instanceId={instance.id} runtimeAvailable={running} />
        </Card>
      </PageHost> },
        { key: 'resources', label: t('desktop.title'), children: <PageHost name="workbench-resources" active={activeSection === 'resources'}>
        {running ? <DesktopResources instanceId={instance.id} runtime={instance.runtime} onOpenMcp={() => openSection('top')} /> :
          <Card><Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('computer.redesign.desktopWaiting')} /></Card>}
      </PageHost> },
        { key: 'diagnostics', label: t('computer.redesign.diagnostics'), children: <PageHost name="workbench-diagnostics" active={activeSection === 'diagnostics'}>
        <ComputerWorkbenchDiagnostics instanceId={instance.id}
          activeTab={diagnosticTab} onTabChange={setDiagnosticTab} />
      </PageHost> }
      ]} />
    </main>
  </div>;
}
