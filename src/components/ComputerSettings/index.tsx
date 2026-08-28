import { useEffect, useMemo, useState } from 'react';
import {
  Button,
  Card,
  Empty,
  Menu,
  Skeleton,
  Space,
  Typography,
} from 'antd';
import {
  ApiOutlined,
  AppstoreOutlined,
  ArrowLeftOutlined,
  DesktopOutlined,
  FormOutlined,
  ReadOutlined,
  RobotOutlined,
  ToolOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { InputVariables } from '@/components/InputVariables';
import { MarketplaceTab } from '@/components/Computer/MarketplaceTab';
import { McpConfig } from '@/components/McpConfig';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import { useComputerStore } from '@/stores/computerStore';
import {
  computerSettingsNavigationKey,
  toComputerSettingsSection,
  type ComputerSettingsSection,
  type PluginSettingsTarget,
} from '@/components/Computer/tabs';
import { GeneralSettings } from './GeneralSettings';
import { SkillsSettings } from './SkillsSettings';
import { BuiltInToolsSettings } from './BuiltInToolsSettings';
import styles from './ComputerSettings.module.css';

const { Paragraph, Text, Title } = Typography;

interface ComputerSettingsProps {
  initialSection?: ComputerSettingsSection;
  targetPlugin?: PluginSettingsTarget | null;
  onNavigate?: (key: string) => void;
}

export function ComputerSettings({
  initialSection = 'general',
  targetPlugin = null,
  onNavigate,
}: ComputerSettingsProps) {
  const { t } = useTranslation();
  const [activeSection, setActiveSection] = useState<ComputerSettingsSection>(initialSection);
  const [focusedPlugin, setFocusedPlugin] = useState<PluginSettingsTarget | null>(targetPlugin);
  const {
    instances,
    loading,
    selectedInstanceId,
    fetchInstances,
  } = useComputerStore();

  useEffect(() => {
    void fetchInstances();
  }, [fetchInstances]);

  useEffect(() => {
    setActiveSection(initialSection);
  }, [initialSection]);

  useEffect(() => {
    setFocusedPlugin(targetPlugin);
  }, [targetPlugin]);

  const selectedInstance = instances.find((instance) => instance.id === selectedInstanceId)
    ?? instances[0];

  const sections = useMemo(() => [
    { key: 'general', icon: <DesktopOutlined /> },
    { key: 'skills', icon: <ReadOutlined /> },
    { key: 'plugins', icon: <AppstoreOutlined /> },
    { key: 'mcp', icon: <ApiOutlined /> },
    { key: 'inputs', icon: <FormOutlined /> },
    { key: 'connection', icon: <RobotOutlined /> },
    { key: 'built-in-tools', icon: <ToolOutlined /> },
  ] satisfies Array<{ key: ComputerSettingsSection; icon: React.ReactNode }>, []);

  const menuItems = sections.map(({ key, icon }) => ({
    key,
    icon,
    label: t(`computer.settings.sections.${key}.title`),
  }));
  const sectionTitle = t(`computer.settings.sections.${activeSection}.title`);
  const sectionDescription = t(`computer.settings.sections.${activeSection}.description`);

  const navigateToSection = (key: string) => {
    const section = toComputerSettingsSection(key);
    setActiveSection(section);
    setFocusedPlugin(null);
    onNavigate?.(computerSettingsNavigationKey(section));
  };

  const renderSection = () => {
    if (!selectedInstance) return null;
    switch (activeSection) {
      case 'general':
        return <GeneralSettings instance={selectedInstance} />;
      case 'skills':
        return <SkillsSettings instance={selectedInstance} onNavigate={onNavigate} />;
      case 'plugins':
        return (
          <MarketplaceTab
            instanceId={selectedInstance.id}
            targetPlugin={focusedPlugin}
            onTargetPluginConsumed={() => setFocusedPlugin(null)}
          />
        );
      case 'mcp':
        return (
          <McpConfig
            instanceId={selectedInstance.id}
            onOpenPlugin={(owner) => {
              setFocusedPlugin(owner);
              setActiveSection('plugins');
              onNavigate?.(computerSettingsNavigationKey('plugins', owner));
            }}
          />
        );
      case 'inputs':
        return <InputVariables instanceId={selectedInstance.id} />;
      case 'connection':
        return (
          <RobotConnectionPanel
            instanceId={selectedInstance.id}
            onNavigate={onNavigate}
          />
        );
      case 'built-in-tools':
        return <BuiltInToolsSettings instance={selectedInstance} />;
      default:
        return null;
    }
  };

  if (loading && instances.length === 0) {
    return <Skeleton active paragraph={{ rows: 8 }} />;
  }

  if (!selectedInstance) {
    return (
      <Empty description={t('computer.settings.noComputer')}>
        <Button onClick={() => onNavigate?.('computer')}>
          {t('computer.backToList')}
        </Button>
      </Empty>
    );
  }

  return (
    <div className={styles.page} aria-label={t('computer.settings.pageLabel')}>
      <div className={styles.header}>
        <Button
          icon={<ArrowLeftOutlined />}
          aria-label={t('computer.settings.backToComputer')}
          onClick={() => onNavigate?.('computer-detail:runtime')}
        >
          {t('computer.settings.backToComputer')}
        </Button>
        <Space direction="vertical" size={0} className={styles.identity}>
          <Title level={3} style={{ margin: 0 }}>
            {t('computer.settings.title', { name: selectedInstance.name })}
          </Title>
          <Text type="secondary" copyable>{selectedInstance.id}</Text>
        </Space>
      </div>

      <div className={styles.shell}>
        <Card size="small" className={styles.navigation}>
          <Menu
            mode="inline"
            selectedKeys={[activeSection]}
            items={menuItems}
            onClick={({ key }) => navigateToSection(key)}
            aria-label={t('computer.settings.navigationLabel')}
          />
        </Card>

        <Card className={styles.content}>
          <div className={styles.moduleHeader}>
            <Title level={4} style={{ marginBottom: 4 }}>{sectionTitle}</Title>
            <Paragraph type="secondary" style={{ margin: 0 }}>
              {sectionDescription}
            </Paragraph>
          </div>
          {renderSection()}
        </Card>
      </div>
    </div>
  );
}
