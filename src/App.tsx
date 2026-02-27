import { Layout, Menu, Typography } from 'antd';
import {
  SettingOutlined,
  ApiOutlined,
  FileTextOutlined,
  CloudServerOutlined,
  DashboardOutlined,
  FormOutlined,
  BugOutlined,
  DesktopOutlined,
} from '@ant-design/icons';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './styles/App.module.css';
import { McpConfig } from './components/McpConfig';
import { InputVariables } from './components/InputVariables';
import { SmcpConnection } from './components/SmcpConnection';
import { DebugPanel } from './components/DebugPanel';
import { DesktopResources } from './components/DesktopResources';

const { Header, Sider, Content } = Layout;
const { Title } = Typography;

function App() {
  const { t } = useTranslation();
  const [selectedKey, setSelectedKey] = useState('mcp');

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
        return (
          <div>
            <Title level={4}>{t('nav.dashboard')}</Title>
            <p>{t('dashboard.description')}</p>
          </div>
        );
      case 'mcp':
        return <McpConfig />;
      case 'inputs':
        return <InputVariables />;
      case 'smcp':
        return <SmcpConnection />;
      case 'resources':
        return <DesktopResources />;
      case 'debug':
        return <DebugPanel />;
      case 'logs':
        return (
          <div>
            <Title level={4}>{t('logs.title')}</Title>
            <p>{t('logs.description')}</p>
          </div>
        );
      case 'settings':
        return (
          <div>
            <Title level={4}>{t('settings.title')}</Title>
            <p>{t('settings.description')}</p>
          </div>
        );
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
