import { Layout, Menu, Typography, Button, Space } from 'antd';
import {
  SettingOutlined,
  ApiOutlined,
  FileTextOutlined,
  CloudServerOutlined,
  DashboardOutlined,
  FormOutlined,
  BugOutlined,
  DesktopOutlined,
  SunOutlined,
  MoonOutlined,
  UserOutlined,
} from '@ant-design/icons';
import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './styles/App.module.css';
import { McpConfig } from './components/McpConfig';
import { InputVariables } from './components/InputVariables';
import { SmcpConnection } from './components/SmcpConnection';
import { DebugPanel } from './components/DebugPanel';
import { DesktopResources } from './components/DesktopResources';
import { Dashboard } from './components/Dashboard';
import { LogViewer } from './components/LogViewer';
import { Settings } from './components/Settings';
import { ManagerAccount } from './components/ManagerAccount';
import { useThemeStore } from './stores/themeStore';

const { Header, Sider, Content } = Layout;
const { Title } = Typography;

function App() {
  const { t, i18n } = useTranslation();
  const [selectedKey, setSelectedKey] = useState('dashboard');
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
      ],
    },
    {
      key: 'config-group',
      label: t('nav.config'),
      type: 'group' as const,
      children: [
        {
          key: 'mcp',
          icon: <ApiOutlined />,
          label: t('mcp.servers'),
        },
        {
          key: 'inputs',
          icon: <FormOutlined />,
          label: t('inputs.title'),
        },
      ],
    },
    {
      key: 'connection-group',
      label: t('nav.connection'),
      type: 'group' as const,
      children: [
        {
          key: 'manager',
          icon: <UserOutlined />,
          label: t('managerAccount.navLabel'),
        },
        {
          key: 'smcp',
          icon: <CloudServerOutlined />,
          label: t('connection.smcpServer'),
        },
        {
          key: 'resources',
          icon: <DesktopOutlined />,
          label: t('resources.title'),
        },
      ],
    },
    {
      key: 'dev-group',
      label: t('nav.development'),
      type: 'group' as const,
      children: [
        {
          key: 'debug',
          icon: <BugOutlined />,
          label: t('nav.debugPanel'),
        },
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
    switch (selectedKey) {
      case 'dashboard':
        return <Dashboard onNavigate={setSelectedKey} />;
      case 'mcp':
        return <McpConfig />;
      case 'inputs':
        return <InputVariables />;
      case 'manager':
        return <ManagerAccount />;
      case 'smcp':
        return <SmcpConnection />;
      case 'resources':
        return <DesktopResources />;
      case 'debug':
        return <DebugPanel />;
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
            selectedKeys={[selectedKey]}
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
