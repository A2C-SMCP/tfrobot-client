import { Layout, Menu, Typography, Button, Space } from 'antd';
import {
  SettingOutlined,
  FileTextOutlined,
  DashboardOutlined,
  DesktopOutlined,
  SunOutlined,
  MoonOutlined,
  ApiOutlined,
} from '@ant-design/icons';
import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './styles/App.module.css';
import { Dashboard } from './components/Dashboard';
import { LogViewer } from './components/LogViewer';
import { Settings } from './components/Settings';
import { RobotConnections } from './components/RobotConnections';
import { Computer } from './components/Computer';
import { toComputerDetailTab } from './components/Computer/tabs';
import { useThemeStore } from './stores/themeStore';

const { Header, Sider, Content } = Layout;
const { Title } = Typography;

function App() {
  const { t, i18n } = useTranslation();
  const [selectedKey, setSelectedKey] = useState('dashboard');
  const menuSelectedKey = selectedKey.startsWith('computer-detail') ? 'computer' : selectedKey;
  const { resolved, setMode, initFromSettings } = useThemeStore();

  // Initialize theme from persisted settings
  useEffect(() => {
    initFromSettings();
  }, []);

  // Listen for system theme changes
  useEffect(() => {
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const handler = () => {
      const { mode: currentMode } = useThemeStore.getState();
      if (currentMode === 'system') {
        useThemeStore.setState({ resolved: mediaQuery.matches ? 'dark' : 'light' });
      }
    };
    mediaQuery.addEventListener('change', handler);
    return () => mediaQuery.removeEventListener('change', handler);
  }, []);

  const menuItems = [
    {
      key: 'overview-group',
      label: t('nav.overview'),
      type: 'group' as const,
      children: [
        {
          key: 'dashboard',
          icon: <DashboardOutlined />,
          label: t('nav.dashboard'),
        },
        {
          key: 'computer',
          icon: <DesktopOutlined />,
          label: t('computer.title'),
        },
      ],
    },
    {
      key: 'connection-group',
      label: t('nav.connectionLayer'),
      type: 'group' as const,
      children: [
        {
          key: 'robot-connections',
          icon: <ApiOutlined />,
          label: t('nav.robotConnections'),
        },
      ],
    },
    {
      key: 'dev-group',
      label: t('nav.development'),
      type: 'group' as const,
      children: [
        {
          key: 'logs',
          icon: <FileTextOutlined />,
          label: t('logs.title'),
        },
      ],
    },
    {
      key: 'system-group',
      label: t('nav.system'),
      type: 'group' as const,
      children: [
        {
          key: 'settings',
          icon: <SettingOutlined />,
          label: t('settings.title'),
        },
      ],
    },
  ];

  const renderContent = () => {
    const [pageKey, rawDetailTab] = selectedKey.split(':');
    const detailTab = toComputerDetailTab(rawDetailTab);

    switch (pageKey) {
      case 'dashboard':
        return <Dashboard onNavigate={setSelectedKey} />;
      case 'computer':
        return <Computer key="computer-list" />;
      case 'computer-detail':
        return <Computer key={`computer-detail-${detailTab}`} initialView="detail" initialTab={detailTab} />;
      case 'robot-connections':
        return <RobotConnections />;
      case 'logs':
        return <LogViewer />;
      case 'settings':
        return <Settings />;
      default:
        return null;
    }
  };

  return (
    <Layout className={styles.layout}>
      <Header className={styles.header}>
        <Title level={4} className={styles.title}>
          {t('app.name')}
        </Title>
        <Space>
          <Button
            type="text"
            className={styles.headerBtn}
            icon={resolved === 'dark' ? <SunOutlined /> : <MoonOutlined />}
            onClick={() => setMode(resolved === 'dark' ? 'light' : 'dark')}
          />
          <Button
            type="text"
            className={styles.headerBtn}
            onClick={() => i18n.changeLanguage(i18n.language === 'zh' ? 'en' : 'zh')}
          >
            {i18n.language === 'zh' ? 'EN' : '中'}
          </Button>
        </Space>
      </Header>
      <Layout>
        <Sider width={200} className={styles.sider}>
          <Menu
            mode="inline"
            selectedKeys={[menuSelectedKey]}
            items={menuItems}
            onClick={({ key }) => setSelectedKey(key)}
            className={styles.menu}
          />
        </Sider>
        <Content className={styles.content}>
          <div className={styles.contentInner}>
            {renderContent()}
          </div>
        </Content>
      </Layout>
    </Layout>
  );
}

export default App;
