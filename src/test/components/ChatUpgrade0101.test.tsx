import { createServer } from 'node:http';
import { Server } from 'socket.io';
import { ChatProvider, createTFRobotChatClient } from '@turingfocus/chat-kit';
import { it, expect } from 'vitest';
import { CompactChatWorkspace } from '@/components/Chat';
import { chatUiLabels } from '@/components/Chat/chatBridge';
import { PageHost } from '@/components/Navigation/PageHost';
import i18n from '@/i18n';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';

it('scopes the published failure draft confirmation to its retained page', async () => {
  await i18n.changeLanguage('en');
  const server = createServer((request, response) => {
    const path = new URL(request.url!, 'http://fixture').pathname;
    const data = path.endsWith('/status') ? { working: false, task_id: null }
      : path.endsWith('/messages') ? {
        messages: [], events: [], cursor: null,
        errors: [{ id: 'failure-1', conversationId: '42', createTimestamp: 20,
          error: 'Fixture limit', code: null,
          message: { msgId: 'original', conversationId: '42', createTimestamp: 10,
            msgType: 'text', role: 'user', content: 'Original text' } }],
      } : { conversations: [{ conversationId: 42, title: 'Fixture' }], cursor: null };
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ code: 200, message: 'Success', data }));
  });
  const sockets = new Server(server, { transports: ['websocket'] });
  sockets.of('/chat').on('connection', (socket) => socket.on('join_conversation', (_data, ack) => ack()));
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${(server.address() as { port: number }).port}`;
  const client = createTFRobotChatClient({
    baseUrl: origin, socketNamespaceUrl: `${origin}/chat`, platformId: 1,
    conversationContract: 'json-v2', serverProfile: { kind: 'current-server' },
    sessionProvider: { getSession: () => ({ kind: 'bearer', token: 'fixture' }) },
    messageCreatorProvider: () => ({ uid: 'user', name: 'User' }),
  });
  const page = (active: boolean) => <PageHost name="chat" active={active}>
    <ChatProvider client={client}><CompactChatWorkspace labels={chatUiLabels((key) => i18n.t(key))} /></ChatProvider>
  </PageHost>;
  const view = render(page(true));
  try {
    await screen.findByRole('button', { name: 'Restore draft' });
    act(() => { client.setComposerDraft({ conversationId: '42', text: 'Keep current draft' }); });
    fireEvent.click(screen.getByRole('button', { name: 'Restore draft' }));
    const dialog = await screen.findByRole('dialog', { name: 'Replace current draft?' });
    expect(dialog.closest('[data-page="chat"]')).toBeTruthy();
    view.rerender(page(false));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(client.getComposerDraft('42').text).toBe('Keep current draft');
    view.rerender(page(true));
    await screen.findByRole('dialog', { name: 'Replace current draft?' });
    fireEvent.click(screen.getByRole('button', { name: 'Replace draft' }));
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Original text'));
  } finally {
    view.unmount();
    await client.dispose({ deadlineAt: Date.now() + 5000 });
    server.closeAllConnections();
    await new Promise<void>((resolve) => sockets.close(() => resolve()));
  }
}, 30_000);
