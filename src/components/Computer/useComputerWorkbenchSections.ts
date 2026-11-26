import { useEffect } from 'react';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { usePageActive } from '@/components/Navigation/pageActivityState';
import type { ComputerWorkbenchSection } from './tabs';

type WorkbenchTab = Exclude<ComputerWorkbenchSection, 'tools'>;
const tabFor = (section: ComputerWorkbenchSection): WorkbenchTab => section === 'tools' ? 'skills' : section;

/** Per-Computer task selection survives settings navigation and subtree release. */
export function useComputerWorkbenchSections(initialSection: ComputerWorkbenchSection, navigationRevision = 0) {
  const active = usePageActive();
  const [savedSection, setActiveSection] = useNavigationState<WorkbenchTab | 'diagnostics'>('workbench.section', tabFor(initialSection));
  const activeSection = savedSection === 'diagnostics' ? 'logs' : savedSection;
  const [capability, setCapability] = useNavigationState<'skills' | 'tools'>('workbench.capability', initialSection === 'tools' ? 'tools' : 'skills');
  const [appliedRevision, setAppliedRevision] = useNavigationState<number | null>('workbench.navigationRevision', null);
  const openSection = (section: WorkbenchTab) => {
    if (!active) return;
    setActiveSection(section);
  };
  useEffect(() => {
    if (!active || appliedRevision === navigationRevision) return;
    setAppliedRevision(navigationRevision);
    if (initialSection === 'tools' || initialSection === 'skills') setCapability(initialSection);
    setActiveSection(tabFor(initialSection));
  }, [active, appliedRevision, initialSection, navigationRevision, setActiveSection, setAppliedRevision, setCapability]);
  return { activeSection, capability, setCapability, openSection };
}
