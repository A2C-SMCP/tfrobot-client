import React from 'react';
import ReactDOM from 'react-dom/client';
import { App as AntdApp, ConfigProvider } from 'antd';
import enUS from 'antd/locale/en_US';
import zhCN from 'antd/locale/zh_CN';
import { useTranslation } from 'react-i18next';
import App from './App';
import { clientTheme } from './styles/theme';
import { useThemeStore } from './stores/themeStore';
import { installUiNoticeFlushHooks } from './stores/uiNoticeStore';
import { initLogger } from './utils/logger';
import './i18n';
import './styles/index.css';

initLogger().catch(console.error);
// Batched notice impressions are written when the window is hidden or torn down.
installUiNoticeFlushHooks();

function Root() {
  const { i18n } = useTranslation();
  const language = i18n.resolvedLanguage ?? i18n.language;
  const resolved = useThemeStore((s) => s.resolved);
  React.useLayoutEffect(() => {
    document.documentElement.style.colorScheme = resolved;
    document.documentElement.lang = language;
  }, [resolved, language]);

  return (
    <ConfigProvider
      locale={language.startsWith('zh') ? zhCN : enUS}
      theme={clientTheme(resolved === 'dark')}
    >
      {/* antd <App> provides context for Modal/message/notification hooks,
          so App.useApp() consumers resolve theme tokens correctly (esp. dark). */}
      <AntdApp className="client-app" style={{ colorScheme: resolved }}>
        <App />
      </AntdApp>
    </ConfigProvider>
  );
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>
);
