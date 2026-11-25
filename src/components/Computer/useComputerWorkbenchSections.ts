import { useEffect } from 'react';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { usePageActive } from '@/components/Navigation/pageActivityState';
import type { ComputerWorkbenchSection } from './tabs';

export type WorkbenchTab = 'top' | 'skills' | 'resources' | 'diagnostics';
export type DiagnosticTab = 'debug' | 'logs';
const tabFor = (section: ComputerWorkbenchSection): WorkbenchTab => section === 'debug' || section === 'logs' ? 'diagnostics' : section;

/** Per-Computer task selection survives settings navigation and subtree release. */
export function useComputerWorkbenchSections(initialSection: ComputerWorkbenchSection, navigationRevision = 0) {
  const active = usePageActive();
  const [activeSection, setActiveSection] = useNavigationState<WorkbenchTab>('workbench.section', tabFor(initialSection));
  const [savedDiagnosticTab, setDiagnosticTab] = useNavigationState('workbench.diagnosticTab', initialSection === 'debug' ? 'debug' : 'logs');
  const diagnosticTab: DiagnosticTab = savedDiagnosticTab === 'debug' ? 'debug' : 'logs';
  const [appliedRevision, setAppliedRevision] = useNavigationState<number | null>('workbench.navigationRevision', null);
  const openSection = (section: ComputerWorkbenchSection) => {
    if (!active) return;
    if (section === 'debug' || section === 'logs') setDiagnosticTab(section);
    setActiveSection(tabFor(section));
  };
  useEffect(() => {
    if (!active || appliedRevision === navigationRevision) return;
    setAppliedRevision(navigationRevision);
    if (initialSection === 'debug' || initialSection === 'logs') setDiagnosticTab(initialSection);
    setActiveSection(tabFor(initialSection));
  }, [active, appliedRevision, initialSection, navigationRevision, setActiveSection, setAppliedRevision, setDiagnosticTab]);
  return { activeSection, diagnosticTab, setDiagnosticTab, openSection };
}
