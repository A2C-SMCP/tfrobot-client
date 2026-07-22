import { Tabs } from 'antd';
import { useTranslation } from 'react-i18next';
import { ToolBrowser } from './ToolBrowser';
import { CallHistory } from './CallHistory';
import { ResourceBrowser } from './ResourceBrowser';

interface DebugPanelProps {
  instanceId: string;
}

export function DebugPanel({ instanceId }: DebugPanelProps) {
  const { t } = useTranslation();

  const items = [
    {
      key: 'tools',
      label: t('debug.tabs.tools'),
      children: <ToolBrowser instanceId={instanceId} />,
    },
    {
      key: 'resources',
      label: t('debug.tabs.resources'),
      children: <ResourceBrowser instanceId={instanceId} />,
    },
    {
      key: 'history',
      label: t('debug.tabs.history'),
      children: <CallHistory instanceId={instanceId} />,
    },
  ];

  return <Tabs defaultActiveKey="tools" items={items} />;
}
