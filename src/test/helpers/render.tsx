import { render, RenderOptions } from '@testing-library/react';
import { App, ConfigProvider } from 'antd';
import { I18nextProvider } from 'react-i18next';
import i18n from '../../i18n';
import { ReactElement } from 'react';

/**
 * Custom render wrapped with Ant Design ConfigProvider + App and I18nextProvider.
 * All component tests should use this instead of raw render. The <App> wrapper
 * provides context for components that use App.useApp() (modal / message /
 * notification hooks), otherwise they emit the "Static function" warning.
 */
export function renderWithProviders(
  ui: ReactElement,
  options?: Omit<RenderOptions, 'wrapper'>
) {
  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <I18nextProvider i18n={i18n}>
        <ConfigProvider>
          <App>{children}</App>
        </ConfigProvider>
      </I18nextProvider>
    );
  }
  return render(ui, { wrapper: Wrapper, ...options });
}

export * from '@testing-library/react';
export { renderWithProviders as render };
