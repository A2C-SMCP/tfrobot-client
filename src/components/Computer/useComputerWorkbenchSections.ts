import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { ComputerWorkbenchSection } from './tabs';

type DiagnosticPanel = Extract<ComputerWorkbenchSection, 'debug' | 'logs'>;

export function useComputerWorkbenchSections(initialSection: ComputerWorkbenchSection) {
  const topRef = useRef<HTMLDivElement>(null);
  const skillsRef = useRef<HTMLDivElement>(null);
  const resourcesRef = useRef<HTMLDivElement>(null);
  const debugRef = useRef<HTMLDivElement>(null);
  const logsRef = useRef<HTMLDivElement>(null);
  const [diagnosticPanels, setDiagnosticPanels] = useState<DiagnosticPanel[]>([]);
  const sectionRefs = useMemo(() => ({
    top: topRef,
    skills: skillsRef,
    resources: resourcesRef,
    debug: debugRef,
    logs: logsRef,
  }), []);

  const openSection = useCallback((section: ComputerWorkbenchSection) => {
    if (section === 'debug' || section === 'logs') {
      setDiagnosticPanels((current) => current.includes(section)
        ? current
        : [...current, section]);
    }
    requestAnimationFrame(() => {
      const target = sectionRefs[section].current;
      target?.scrollIntoView?.({ behavior: 'smooth', block: 'start' });
      target?.focus?.({ preventScroll: true });
    });
  }, [sectionRefs]);

  const changeDiagnosticPanels = useCallback((keys: string | string[]) => {
    const panels = (Array.isArray(keys) ? keys : [keys])
      .filter((key): key is DiagnosticPanel => key === 'debug' || key === 'logs');
    setDiagnosticPanels(panels);
  }, []);

  useEffect(() => {
    if (initialSection !== 'top') {
      openSection(initialSection);
    }
  }, [initialSection, openSection]);

  return {
    sectionRefs,
    diagnosticPanels,
    changeDiagnosticPanels,
    openSection,
  };
}
