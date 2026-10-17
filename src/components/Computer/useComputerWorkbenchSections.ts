import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { usePageActive, usePageAction } from '@/components/Navigation/pageActivityState';
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
} from 'react';
import type { ComputerWorkbenchSection } from './tabs';

export function useComputerWorkbenchSections(initialSection: ComputerWorkbenchSection, navigationRevision = 0) {
  const active = usePageActive();
  const action = usePageAction();
  const frame = useRef<number | null>(null);
  useEffect(() => () => {
    if (frame.current !== null) cancelAnimationFrame(frame.current);
  }, [active]);
  const [appliedRevision, setAppliedRevision] = useNavigationState<number | null>('workbench.navigationRevision', null);
  const topRef = useRef<HTMLDivElement>(null);
  const skillsRef = useRef<HTMLDivElement>(null);
  const resourcesRef = useRef<HTMLDivElement>(null);
  const debugRef = useRef<HTMLDivElement>(null);
  const logsRef = useRef<HTMLDivElement>(null);
  const sectionRefs = useMemo(() => ({
    top: topRef,
    skills: skillsRef,
    resources: resourcesRef,
    debug: debugRef,
    logs: logsRef,
  }), []);

  const openSection = useCallback((section: ComputerWorkbenchSection) => {
    if (!active) return;
    const current = action();
    if (frame.current !== null) cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      if (!current()) return;
      const target = sectionRefs[section].current;
      // A browser smooth-scroll animation can continue after this page hides
      // and overwrite the destination page's restored window position.
      target?.scrollIntoView?.({ behavior: 'instant', block: 'start' });
      target?.focus?.({ preventScroll: true });
    });
  }, [active, action, sectionRefs]);

  useEffect(() => {
    if (!active || appliedRevision === navigationRevision) return;
    setAppliedRevision(navigationRevision);
    if (initialSection !== 'top' || navigationRevision > 0) openSection(initialSection);
  }, [active, appliedRevision, initialSection, navigationRevision, openSection, setAppliedRevision]);

  return {
    sectionRefs,
    openSection,
  };
}
