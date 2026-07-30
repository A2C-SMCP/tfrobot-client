import React from 'react';
import ReactDOM from 'react-dom/client';
import { App as AntdApp, theme as antTheme, ConfigProvider } from 'antd';
import App from './App';
import { useThemeStore } from './stores/themeStore';
import { initLogger } from './utils/logger';
import './i18n';
import './styles/index.css';

initLogger().catch(console.error);

function Root() {
  const resolved = useThemeStore((s) => s.resolved);

  return (
    <ConfigProvider
      theme={{
        algorithm:
          resolved === 'dark' ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
        token: {
          colorPrimary: '#1890ff',
        },
        components: {
          Menu: {
            groupTitleColor: resolved === 'dark'
              ? 'rgba(255, 255, 255, 0.85)'
              : undefined,
          },
        },
      }}
    >
      {/* antd <App> provides context for Modal/message/notification hooks,
          so App.useApp() consumers resolve theme tokens correctly (esp. dark). */}
      <AntdApp>
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
