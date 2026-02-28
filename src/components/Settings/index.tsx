import { useEffect } from 'react';
import { Tabs, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';
import { AppearanceSettings } from './AppearanceSettings';
import { RuntimeSettings } from './RuntimeSettings';
import { DataSettings } from './DataSettings';
import { AboutSection } from './AboutSection';

const { Title } = Typography;

export function Settings() {
  const { t } = useTranslation();
  const { fetchSettings } = useSettingsStore();

  useEffect(() => {
    fetchSettings();
  }, []);

  return (
    <div>
      <Title level={4}>{t('settings.title')}</Title>
      <Tabs
        defaultActiveKey="appearance"
        items={[
          { key: 'appearance', label: t('settings.appearance'), children: <AppearanceSettings /> },
          { key: 'runtime', label: t('settings.runtime'), children: <RuntimeSettings /> },
          { key: 'data', label: t('settings.data'), children: <DataSettings /> },
          { key: 'about', label: t('settings.about'), children: <AboutSection /> },
        ]}
      />
    </div>
  );
}
