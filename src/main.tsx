import React from 'react';
import ReactDOM from 'react-dom/client';
import { theme as antTheme, ConfigProvider } from 'antd';
import App from './App';
import { useThemeStore } from './stores/themeStore';
import './i18n';
import './styles/index.css';

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
      }}
    >
      <App />
    </ConfigProvider>
  );
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>
);
