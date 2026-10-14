import { PageHost } from '@/components/Navigation/PageHost';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { useEffect } from 'react';
import { Tabs, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';
import { AppearanceSettings } from './AppearanceSettings';
import { RuntimeSettings } from './RuntimeSettings';
import { DataSettings } from './DataSettings';
import { PermissionsSettings } from './PermissionsSettings';
import { AboutSection } from './AboutSection';
import { settingsNavigationKey, toSettingsTab, type SettingsTab } from './tabs';
import type { PermissionAnchor } from './permissions';

const { Title } = Typography;

interface SettingsProps {
  initialTab?: SettingsTab;
  navigationRevision?: number;
  /** Anchor inside the permissions tab that a notice asked to open. */
  focusAnchor?: PermissionAnchor | null;
  onNavigate?: (key: string) => void;
}

export function Settings({
  initialTab = 'appearance',
  navigationRevision = 0,
  focusAnchor = null,
  onNavigate,
}: SettingsProps) {
  const { t } = useTranslation();
  const { fetchSettings } = useSettingsStore();
  const [activeTab, setActiveTab] = useNavigationState<SettingsTab>('settings.tab', initialTab);
  const [appliedRevision, setAppliedRevision] = useNavigationState(
    'settings.navigationRevision',
    navigationRevision,
  );

  useEffect(() => {
    fetchSettings();
  }, [fetchSettings]);

  // A second navigation to the same tab must still apply (e.g. another notice anchor).
  useEffect(() => {
    if (appliedRevision === navigationRevision) return;
    setAppliedRevision(navigationRevision);
    setActiveTab(initialTab);
  }, [appliedRevision, initialTab, navigationRevision, setActiveTab, setAppliedRevision]);

  const changeTab = (key: string) => {
    const tab = toSettingsTab(key);
    setActiveTab(tab);
    onNavigate?.(settingsNavigationKey(tab));
  };

  return (
    <div>
      <Title level={4}>{t('settings.title')}</Title>
      <Tabs
        activeKey={activeTab}
        onChange={changeTab}
        items={[
          { key: 'appearance', label: t('settings.appearance'), children: <PageHost name="settings-tab-appearance" active={activeTab === 'appearance'}><AppearanceSettings /></PageHost> },
          { key: 'runtime', label: t('settings.runtime'), children: <PageHost name="settings-tab-runtime" active={activeTab === 'runtime'}><RuntimeSettings /></PageHost> },
          { key: 'data', label: t('settings.data'), children: <PageHost name="settings-tab-data" active={activeTab === 'data'}><DataSettings /></PageHost> },
          { key: 'permissions', label: t('settings.permissions'), children: <PageHost name="settings-tab-permissions" active={activeTab === 'permissions'}><PermissionsSettings focusAnchor={focusAnchor} focusRevision={navigationRevision} /></PageHost> },
          { key: 'about', label: t('settings.about'), children: <PageHost name="settings-tab-about" active={activeTab === 'about'}><AboutSection /></PageHost> },
        ]}
      />
    </div>
  );
}
