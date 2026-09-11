import { retireViewContext } from '@/stores/viewContextLifetime';
import { useDesktopStore } from '@/stores/desktopStore';
import { useSkillStore } from '@/stores/skillStore';
import { useSdkConfigStore } from '@/stores/sdkConfigStore';
import { useInputStore } from '@/stores/inputStore';
import { useMcpStore } from '@/stores/mcpStore';
import { useDebugStore } from '@/stores/debugStore';
import { pruneActivityViews, resetActivityViews } from '@/stores/activityStore';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { createNavigationStore, NavigationMemory } from '@/stores/navigationStore';
import { useManagerStore } from '@/stores/managerStore';
import { useComputerStore } from '@/stores/computerStore';
import { ActivityViewer } from '@/components/ActivityViewer';
import { Chat } from '@/components/Chat';
import { Computer } from '@/components/Computer';
import { ComputerSettings } from '@/components/ComputerSettings';
import { RobotConnections } from '@/components/RobotConnections';
import { Settings } from '@/components/Settings';
import { legacyComputerSettingsSection, parsePluginSettingsTarget, toComputerSettingsSection, toComputerWorkbenchSection } from '@/components/Computer/tabs';
import { PageHost } from './PageHost';
import { NavigationMemoryProvider, NavigationScope } from './NavigationMemory';

type Navigation = ReturnType<typeof createNavigationStore>;

export function NavigationPages({ navigation }: { navigation: Navigation }) {
  const { context } = useManagerStore();
  const identity = JSON.stringify([context.authState, context.contextKey, context.user?.id, [...context.permissions].sort()]);
  return <ScopedPages key={identity} navigation={navigation} />;
}

function ScopedPages({ navigation }: { navigation: Navigation }) {
  const { route, requests } = useStore(navigation);
  const [memory] = useState(() => new NavigationMemory());
  const previousSelection = useRef<string | null>(null);
  const selectedInstanceId = useComputerStore((state) => state.selectedInstanceId);
  const instances = useComputerStore((state) => state.instances);
  const navigate = useCallback((next: string) => {
    const [page, section] = next.split(':');
    const legacy = page === 'computer-detail' ? legacyComputerSettingsSection(section) : null;
    navigation.getState().navigate(legacy ? `computer-settings:${legacy}` : next);
  }, [navigation]);
  const page = route.split(':')[0];
  const detailRequest = requests['computer-detail'];
  const listRequest = requests.computer;
  const computerRequest = (listRequest?.revision ?? 0) > (detailRequest?.revision ?? 0) ? listRequest : detailRequest;
  const settingsRequest = requests['computer-settings'];
  const [, rawSettingsSection] = (settingsRequest?.route ?? '').split(':');
  const targetPlugin = useMemo(() => parsePluginSettingsTarget((settingsRequest?.route ?? '').split(':').slice(2)), [settingsRequest]);
  const settingsSection = toComputerSettingsSection(rawSettingsSection);

  useEffect(() => {
    const ids = new Set(instances.map((instance) => instance.id));
    memory.pruneComputers(ids);
    pruneActivityViews(ids);
    if (previousSelection.current && !ids.has(previousSelection.current)) navigate('computer');
    previousSelection.current = selectedInstanceId;
    const skills = useSkillStore.getState().recordsByInstanceId;
    if (Object.keys(skills).some((id) => !ids.has(id))) {
      useSkillStore.setState({ recordsByInstanceId: Object.fromEntries(Object.entries(skills).filter(([id]) => ids.has(id))) });
    }
    const desktops = useDesktopStore.getState().instances;
    if (Object.keys(desktops).some((id) => !ids.has(id))) {
      useDesktopStore.setState({ instances: Object.fromEntries(Object.entries(desktops).filter(([id]) => ids.has(id))) });
    }
  }, [instances, memory, navigate, selectedInstanceId]);
  useEffect(() => () => {
    retireViewContext();
    memory.clear();
    resetActivityViews();
    useDebugStore.getState().reset();
    useMcpStore.getState().reset();
    useInputStore.getState().reset();
    useSdkConfigStore.getState().reset();
    useSkillStore.getState().reset();
    useDesktopStore.getState().reset();

    navigation.setState(navigation.getInitialState(), true);
    useComputerStore.setState({ selectedInstanceId: null });
  }, [memory, navigation]);

  return (
    <NavigationMemoryProvider memory={memory}>
      <NavigationScope id="app">
        <PageHost name="chat" active={page === 'chat'}><Chat /></PageHost>
        <PageHost name="computer" active={page === 'computer' || page === 'computer-detail'}>
          <Computer
            initialView={computerRequest?.route.startsWith('computer-detail') ? 'detail' : 'list'}
            initialSection={toComputerWorkbenchSection(computerRequest?.route.split(':')[1])}
            navigationRevision={computerRequest?.revision ?? 0}
            onNavigate={navigate}
          />
        </PageHost>
        <PageHost name="computer-settings" active={page === 'computer-settings'}>
          <ComputerSettings initialSection={settingsSection}
            navigationRevision={settingsRequest?.revision ?? 0}
            targetPlugin={settingsSection === 'plugins' ? targetPlugin : null}
            onNavigate={navigate} />
        </PageHost>
        <PageHost name="robot-connections" active={page === 'robot-connections'}><RobotConnections /></PageHost>
        <PageHost name="logs" active={page === 'logs'}><ActivityViewer /></PageHost>
        <PageHost name="settings" active={page === 'settings'}><Settings /></PageHost>
      </NavigationScope>
    </NavigationMemoryProvider>
  );
}
