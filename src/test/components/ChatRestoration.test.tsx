import { createServer, type ServerResponse } from 'node:http';
import { Server as SocketServer } from 'socket.io';
import { invoke } from '@tauri-apps/api/core';
import { ChatProvider, type ChatClient } from '@turingfocus/chat-kit';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '../helpers/render';
import { CompactChatWorkspace } from '@/components/Chat';
import { chatUiLabels, createClientChatFactory } from '@/components/Chat/chatBridge';
import i18n from '@/i18n';

function envelope(response: ServerResponse, data: unknown, status = 200) {
  response.writeHead(status, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify({ code: status, message: 'Fixture', data }));
}

describe('restart selection using real Chat Kit and HTTP/Socket.IO', () => {
  let http: ReturnType<typeof createServer>;
  let sockets: SocketServer;
  let origin: string;
  let clients: ChatClient[];
  let remembered: string | null;
  let readFails: boolean;
  let writeFails: boolean;
  let statuses: Map<string, number>;
  let loaded: string[];
  let list: number[];
  let held: ServerResponse | undefined;
  let holdId: string | undefined;
  const writes: string[] = [];
  const message = (id: string) => ({ msgId: `message-${id}`, conversationId: Number(id), content: `History ${id}`,
    additionalKwargs: {}, attachments: null, createTimestamp: 1_800_000_000_000,
    creator: { uid: 'agent', name: 'Agent', avatar: null }, role: 'assistant', msgType: 'text' });

  beforeEach(async () => {
    vi.clearAllMocks();
    await i18n.changeLanguage('en');
    clients = []; remembered = '99'; readFails = false; writeFails = false; statuses = new Map();
    loaded = []; list = [42, 43]; writes.length = 0; held = undefined; holdId = undefined;
    http = createServer((request, response) => {
      const path = new URL(request.url ?? '/', 'http://localhost').pathname;
      if (path.endsWith('/status')) return envelope(response, { working: false, task_id: null });
      if (path.endsWith('/messages')) {
        const id = path.match(/conversations\/(\d+)/)?.[1] ?? '';
        loaded.push(id);
        if (holdId === id) { held = response; holdId = undefined; return; }
        if (statuses.has(id)) return envelope(response, null, statuses.get(id));
        return envelope(response, { messages: [message(id)], events: [], cursor: null });
      }
      if (path.endsWith('/conversations')) return envelope(response, {
        conversations: list.map((id) => ({ conversationId: id, title: `Conversation ${id}`, description: null,
          updateTimestamp: 1_800_000_000_000 - id })), cursor: null,
      });
      envelope(response, null, 404);
    });
    sockets = new SocketServer(http, { transports: ['websocket'], cors: { origin: '*' } });
    sockets.of('/chat').on('connection', (socket) => {
      socket.on('join_conversation', (_payload: unknown, ack: () => void) => ack());
    });
    await new Promise<void>((resolve) => http.listen(0, '127.0.0.1', resolve));
    const address = http.address();
    if (!address || typeof address === 'string') throw new Error('Missing address');
    origin = `http://127.0.0.1:${address.port}`;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'chat_get_recent_conversation') {
        if (readFails) throw new Error('Cannot read preference');
        return remembered;
      }
      if (command === 'chat_remember_conversation') {
        if (writeFails) throw new Error('Cannot save preference');
        remembered = (args as { conversationId: string }).conversationId;
        writes.push(remembered); return;
      }
      if (command === 'chat_get_session_token') return { token: 'fixture', expiresAt: Date.now() + 100_000 };
      if (command === 'chat_invalidate_session') return;
      if (command !== 'chat_http_request') throw new Error(`Unexpected IPC ${command}`);
      const { url, method } = args as { url: string; method: string };
      const result = await fetch(url, { method });
      return { status: result.status, body: await result.text(), contentType: 'application/json' };
    });
  });
  afterEach(async () => {
    cleanup(); held?.end();
    await Promise.all(clients.map((client) => client.dispose({ deadlineAt: Date.now() + 5000 })));
    await new Promise<void>((resolve) => sockets.close(() => resolve()));
    http.closeAllConnections();
  });
  function mount(leaseId = 'restart') {
    const client = createClientChatFactory({ descriptor: { leaseId, employeeId: 42, robotName: 'Robot',
      httpBaseUrl: `${origin}/`, socketNamespaceUrl: `${origin}/chat`, socketPath: '/socket.io' },
      messageCreator: { uid: 'user', name: 'User' }, onDiagnostic: vi.fn(), onUnhandledError: vi.fn() }).create();
    clients.push(client);
    return render(<ChatProvider client={client}><CompactChatWorkspace leaseId={leaseId}
      labels={chatUiLabels((key) => i18n.t(key))} /></ChatProvider>);
  }
  async function select(id: number) {
    fireEvent.click(screen.getByRole('button', { name: 'History' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: `Conversation ${id}` }));
  }
  it('restores an ID outside the first page and remembers viewing another history across a fresh client', async () => {
    const view = mount();
    await screen.findByText('History 99');
    expect(new Set(loaded)).toEqual(new Set(['99']));
    await select(43);
    await screen.findByText('History 43');
    await waitFor(() => expect(remembered).toBe('43'));
    view.unmount();
    mount('new-process');
    await screen.findByText('History 43');
    expect(writes.at(-1)).toBe('43');
  });
  it('selects the first valid conversation on first launch', async () => {
    remembered = null; mount();
    await screen.findByText('History 42');
    await waitFor(() => expect(remembered).toBe('42'));
  });
  it.each([404, 403])('falls back only on a definitive unavailable response (%s)', async (status) => {
    statuses.set('99', status); mount();
    await screen.findByText('History 42');
    await waitFor(() => expect(remembered).toBe('42'));
  });
  it('keeps the saved ID on temporary server failure and restores it on retry', async () => {
    statuses.set('99', 503); mount();
    await screen.findByText(/Could not restore or save/);
    expect(remembered).toBe('99'); expect(writes).toEqual([]);
    statuses.clear();
    fireEvent.click(screen.getAllByRole('button', { name: 'Retry' })[0]);
    await screen.findByText('History 99');
  });
  it('allows manual selection after a preference read failure without auto-selecting the first item', async () => {
    readFails = true; mount();
    await screen.findByText(/Could not restore or save/);
    expect(loaded).toEqual([]); expect(writes).toEqual([]);
    await select(43);
    await screen.findByText('History 43');
    await waitFor(() => expect(remembered).toBe('43'));
  });
  it('does not let a late restore override a newer manual selection', async () => {
    holdId = '99'; mount();
    await waitFor(() => expect(held).toBeDefined());
    await select(43);
    await screen.findByText('History 43');
    envelope(held!, { messages: [message('99')], events: [], cursor: null });
    await waitFor(() => expect(remembered).toBe('43'));
    expect(writes).not.toContain('99');
    expect(screen.queryByText('History 99')).not.toBeInTheDocument();
  });
  it('persists only the final successful selection during rapid manual switches', async () => {
    mount();
    await screen.findByText('History 99');
    holdId = '43';
    await select(43);
    await waitFor(() => expect(held).toBeDefined());
    await select(42);
    await screen.findByText('History 42');
    envelope(held!, { messages: [message('43')], events: [], cursor: null });
    await waitFor(() => expect(remembered).toBe('42'));
    expect(writes).not.toContain('43');
    expect(screen.queryByText('History 43')).not.toBeInTheDocument();
  });
  it('retries a failed write without selecting a different conversation', async () => {
    writeFails = true; mount();
    await screen.findByText('History 99');
    await screen.findByText(/Could not restore or save/);
    writeFails = false;
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(writes).toEqual(['99']));
    expect(new Set(loaded)).toEqual(new Set(['99']));
  });
  it('shows an empty state when the saved conversation is gone and the list is empty', async () => {
    statuses.set('99', 404); list = []; mount();
    await waitFor(() => expect(new Set(loaded)).toEqual(new Set(['99'])));
    await screen.findByText('No conversation selected');
    expect(writes).toEqual([]);
  });
});
