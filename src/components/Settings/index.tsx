import { PageHost } from '@/components/Navigation/PageHost';
import { useEffect, useState } from 'react';
import { Tabs, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';
import { AppearanceSettings } from './AppearanceSettings';
import { RuntimeSettings } from './RuntimeSettings';
import { DataSettings } from './DataSettings';
import { AboutSection } from './AboutSection';

const { Title } = Typography;

export function Settings() {
  const [activeTab, setActiveTab] = useState('appearance');
  const { t } = useTranslation();
  const { fetchSettings } = useSettingsStore();

  useEffect(() => {
    fetchSettings();
  }, [fetchSettings]);

  return (
    <div>
      <Title level={4}>{t('settings.title')}</Title>
      <Tabs
        activeKey={activeTab}
        onChange={setActiveTab}
        items={[
          { key: 'appearance', label: t('settings.appearance'), children: <PageHost name="settings-tab-appearance" active={activeTab === 'appearance'}><AppearanceSettings /></PageHost> },
          { key: 'runtime', label: t('settings.runtime'), children: <PageHost name="settings-tab-runtime" active={activeTab === 'runtime'}><RuntimeSettings /></PageHost> },
          { key: 'data', label: t('settings.data'), children: <PageHost name="settings-tab-data" active={activeTab === 'data'}><DataSettings /></PageHost> },
          { key: 'about', label: t('settings.about'), children: <PageHost name="settings-tab-about" active={activeTab === 'about'}><AboutSection /></PageHost> },
        ]}
      />
    </div>
  );
}
