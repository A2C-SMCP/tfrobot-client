import { Tabs } from 'antd';
import { useTranslation } from 'react-i18next';
import { ToolBrowser } from './ToolBrowser';
import { CallHistory } from './CallHistory';
import { ResourceBrowser } from './ResourceBrowser';

export function DebugPanel() {
  const { t } = useTranslation();

  const items = [
    {
      key: 'tools',
      label: t('debug.tabs.tools'),
      children: <ToolBrowser />,
    },
    {
      key: 'resources',
      label: t('debug.tabs.resources'),
      children: <ResourceBrowser />,
    },
    {
      key: 'history',
      label: t('debug.tabs.history'),
      children: <CallHistory />,
    },
  ];

  return <Tabs defaultActiveKey="tools" items={items} />;
}
