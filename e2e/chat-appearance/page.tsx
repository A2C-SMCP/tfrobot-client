import { createRoot } from 'react-dom/client';
import { App, ConfigProvider } from 'antd';
import { ChatProvider, createTFRobotChatClient } from '@turingfocus/chat-kit';
import { ChatCompactWorkspace } from '@turingfocus/chat-ui-antd';
import { CompactChatWorkspace } from '@/components/Chat';
import { chatUiLabels } from '@/components/Chat/chatBridge';
import { chatWorkspaceTheme, clientTheme } from '@/styles/theme';
import { useThemeStore } from '@/stores/themeStore';
import i18n from '@/i18n';

const parameters = new URLSearchParams(location.search);
const dark = parameters.get('dark') === '1';
const official = parameters.get('view') === 'official';
useThemeStore.setState({ resolved: dark ? 'dark' : 'light' });
await i18n.changeLanguage('zh');
const client = createTFRobotChatClient({
  baseUrl: 'http://127.0.0.1:18768/',
  socketNamespaceUrl: 'http://127.0.0.1:18768/chat',
  serverProfile: { kind: 'current-server' },
  sessionProvider: { getSession: () => ({ kind: 'bearer', token: 'local-fixture' }) },
  messageCreatorProvider: () => ({ uid: 'fixture-user', name: 'Fixture' }),
});
const deadline = () => Date.now() + 15_000;

createRoot(document.getElementById('root')!).render(
  <ConfigProvider theme={clientTheme(dark)}>
    <App>
      <main style={{ width: '100%', height: '100vh', minHeight: 0 }}>
        <ChatProvider client={client}>
          {official ? (
            <ConfigProvider theme={chatWorkspaceTheme(dark)} button={{ autoInsertSpace: false }}>
              <App style={{ height: '100%' }}>
                <ChatCompactWorkspace locale="zh-CN" getDeadlineAt={deadline}
                  restoredDraft={{ conversationId: '42', text: '' }} />
              </App>
            </ConfigProvider>
          ) : (
            <CompactChatWorkspace labels={chatUiLabels((key) => i18n.t(key), 'zh')} />
          )}
        </ChatProvider>
      </main>
    </App>
  </ConfigProvider>,
);
