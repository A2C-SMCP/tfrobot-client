import { PageHost } from '@/components/Navigation/PageHost';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { Tabs } from 'antd';
import { useTranslation } from 'react-i18next';
import { ToolBrowser } from './ToolBrowser';
import { CallHistory } from './CallHistory';
import { ResourceBrowser } from './ResourceBrowser';

interface DebugPanelProps {
  instanceId: string;
}

export function DebugPanel({ instanceId }: DebugPanelProps) {
  const [tab, setTab] = useNavigationState('debug.tab', 'tools');
  const { t } = useTranslation();

  const items = [
    {
      key: 'tools',
      label: t('debug.tabs.tools'),
      children: <PageHost name="debug-tab-tools" active={tab === 'tools'}><ToolBrowser instanceId={instanceId} /></PageHost>,
    },
    {
      key: 'resources',
      label: t('debug.tabs.resources'),
      children: <PageHost name="debug-tab-resources" active={tab === 'resources'}><ResourceBrowser instanceId={instanceId} /></PageHost>,
    },
    {
      key: 'history',
      label: t('debug.tabs.history'),
      children: <PageHost name="debug-tab-history" active={tab === 'history'}><CallHistory instanceId={instanceId} /></PageHost>,
    },
  ];

  return <Tabs activeKey={tab} onChange={setTab} items={items} />;
}
