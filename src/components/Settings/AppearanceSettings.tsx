import { Form, Select, Segmented } from 'antd';
import { useTranslation } from 'react-i18next';
import { useThemeStore, type ThemeMode } from '@/stores/themeStore';
import { useSettingsStore } from '@/stores/settingsStore';

export function AppearanceSettings() {
  const { t, i18n } = useTranslation();
  const { mode, setMode } = useThemeStore();
  const { settings, updateSettings } = useSettingsStore();

  const handleLanguageChange = (lang: string) => {
    i18n.changeLanguage(lang);
    if (settings) {
      updateSettings({ ...settings, language: lang });
    }
  };

  return (
    <Form layout="vertical">
      <Form.Item label={t('settings.theme')}>
        <Segmented
          value={mode}
          options={[
            { label: t('settings.themeLight'), value: 'light' },
            { label: t('settings.themeDark'), value: 'dark' },
            { label: t('settings.themeSystem'), value: 'system' },
          ]}
          onChange={(value) => setMode(value as ThemeMode)}
        />
      </Form.Item>

      <Form.Item label={t('settings.language')}>
        <Select
          value={i18n.language}
          onChange={handleLanguageChange}
          style={{ width: 200 }}
          options={[
            { label: '中文', value: 'zh' },
            { label: 'English', value: 'en' },
          ]}
        />
      </Form.Item>
    </Form>
  );
}
