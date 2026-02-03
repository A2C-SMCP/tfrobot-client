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
      label: t('resources.title', 'Resources'),
    },
    {
      key: 'logs',
      icon: <FileTextOutlined />,
      label: t('logs.title', 'Logs'),
    },
    {
      key: 'settings',
      icon: <SettingOutlined />,
      label: t('settings.title'),
    },
  ];

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
            {selectedKey === 'mcp' && (
              <div>
                <Title level={4}>{t('mcp.servers')}</Title>
                <p>{t('mcp.description', 'Manage your MCP servers here.')}</p>
              </div>
            )}
            {selectedKey === 'connection' && (
              <div>
                <Title level={4}>{t('connection.smcpServer')}</Title>
                <p>{t('connection.description', 'Connect to SMCP server.')}</p>
              </div>
            )}
            {selectedKey === 'resources' && (
              <div>
                <Title level={4}>{t('resources.title', 'Resources')}</Title>
                <p>{t('resources.description', 'Browse desktop resources.')}</p>
              </div>
            )}
            {selectedKey === 'logs' && (
              <div>
                <Title level={4}>{t('logs.title', 'Logs')}</Title>
                <p>{t('logs.description', 'View tool call logs.')}</p>
              </div>
            )}
            {selectedKey === 'settings' && (
              <div>
                <Title level={4}>{t('settings.title')}</Title>
                <p>{t('settings.description', 'Configure application settings.')}</p>
              </div>
            )}
          </div>
        </Content>
      </Layout>
    </Layout>
  );
}

export default App;
