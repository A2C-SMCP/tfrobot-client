import { Layout, Menu, Typography } from 'antd';
import {
  SettingOutlined,
  ApiOutlined,
  FolderOutlined,
  FileTextOutlined,
  CloudServerOutlined,
} from '@ant-design/icons';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './styles/App.module.css';
import { McpConfig } from './components/McpConfig';

const { Header, Sider, Content } = Layout;
const { Title } = Typography;

function App() {
  const { t } = useTranslation();
  const [selectedKey, setSelectedKey] = useState('mcp');

  const menuItems = [
    {
      key: 'mcp',
      icon: <ApiOutlined />,
      label: t('mcp.servers'),
    },
    {
      key: 'connection',
      icon: <CloudServerOutlined />,
      label: t('connection.smcpServer'),
    },
    {
      key: 'resources',
      icon: <FolderOutlined />,
      label: t('resources.title'),
    },
    {
      key: 'logs',
      icon: <FileTextOutlined />,
      label: t('logs.title'),
    },
    {
      key: 'settings',
      icon: <SettingOutlined />,
      label: t('settings.title'),
    },
  ];

  const renderContent = () => {
    switch (selectedKey) {
      case 'mcp':
        return <McpConfig />;
      case 'connection':
        return (
          <div>
            <Title level={4}>{t('connection.smcpServer')}</Title>
            <p>{t('connection.description')}</p>
          </div>
        );
      case 'resources':
        return (
          <div>
            <Title level={4}>{t('resources.title')}</Title>
            <p>{t('resources.description')}</p>
          </div>
        );
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
