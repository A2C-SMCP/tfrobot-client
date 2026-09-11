import { createServer, type ServerResponse } from 'node:http';
import { Server as SocketServer } from 'socket.io';
import { invoke } from '@tauri-apps/api/core';
import { ChatProvider, ChatConversationView, type ChatClient } from '@turingfocus/chat-kit';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '../helpers/render';
import { CompactChatWorkspace } from '@/components/Chat';
import { chatUiLabels, createClientChatFactory } from '@/components/Chat/chatBridge';
import i18n from '@/i18n';

const deadlineAt = () => Date.now() + 10_000;
const dto = (id: number, content = `History ${id}`) => ({
  msgId: `message-${id}`, conversationId: id, content, additionalKwargs: {}, attachments: null,
  createTimestamp: 1_800_000_000_000, creator: { uid: 'agent', name: 'Agent', avatar: null },
  role: 'assistant', msgType: 'text',
});
function envelope(response: ServerResponse, data: unknown, status = 200) {
  response.writeHead(status, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify({ code: status, message: status === 200 ? 'Success' : 'Fixture failure', data }));
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => { resolve = settle; });
  return { promise, resolve };
}

// The native IPC boundary forwards fixture requests to a real HTTP server. Gateway,
// Socket.IO, Runtime, workspace and UI are the installed production implementations.
describe('0.8.1 cache through the client host and real HTTP/Socket.IO', () => {
  let client: ChatClient;
  let http: ReturnType<typeof createServer>;
  let sockets: SocketServer;
  let hold: { id: number; response: ReturnType<typeof deferred<ServerResponse>> } | undefined;
  let heldResponses: ServerResponse[];
  let failure: number;
  let posts: number;
  let origin: string;
  let freshA: boolean;

  beforeEach(async () => {
    vi.clearAllMocks();
    await i18n.changeLanguage('en');
    hold = undefined; heldResponses = []; failure = 0; posts = 0; freshA = false;
    http = createServer((request, response) => {
      const path = new URL(request.url ?? '/', 'http://localhost').pathname;
      if (request.headers.authorization !== 'Bearer fixture-token') return envelope(response, null, 401);
      if (request.method === 'POST') { posts++; return envelope(response, { task_id: 'fixture-run' }); }
      if (path.endsWith('/status')) return envelope(response, { working: false, task_id: null });
      if (path.endsWith('/messages')) {
        const id = Number(path.match(/conversations\/(\d+)/)?.[1]);
        if (hold?.id === id) {
          heldResponses.push(response); hold.response.resolve(response); hold = undefined; return;
        }
        if (failure) return envelope(response, null, failure);
        return envelope(response, { messages: freshA && id === 42 ? [dto(id), { ...dto(id, 'Fresh A'), msgId: 'fresh-a' }] : [dto(id)], events: [], cursor: null });
      }
      if (path.endsWith('/conversations')) return envelope(response, {
        conversations: [42, 43, 44].map((id) => ({ conversationId: id, title: `Conversation ${id}`, description: null, updateTimestamp: 1_800_000_000_000 - id })), cursor: null,
      });
      envelope(response, null, 404);
    });
    sockets = new SocketServer(http, { transports: ['websocket'], cors: { origin: '*' } });
    sockets.of('/chat').on('connection', (socket) => {
      socket.on('join_conversation', (_payload: unknown, acknowledge: () => void) => acknowledge());
    });
    await new Promise<void>((resolve) => http.listen(0, '127.0.0.1', resolve));
    const address = http.address();
    if (!address || typeof address === 'string') throw new Error('No fixture address');
    origin = `http://127.0.0.1:${address.port}`;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'chat_get_session_token') return { token: 'fixture-token', expiresAt: deadlineAt() };
      if (command === 'chat_invalidate_session') return;
      if (command !== 'chat_http_request') throw new Error(`Unexpected IPC: ${command}`);
      const request = args as { url: string; method: string; body?: string };
      const response = await fetch(request.url, {
        method: request.method, body: request.body, headers: { Authorization: 'Bearer fixture-token' },
      });
      return { status: response.status, contentType: 'application/json', body: await response.text() };
    });
    client = createClientChatFactory({
      descriptor: { leaseId: 'cache-lease', employeeId: 42, robotName: 'Robot', httpBaseUrl: `${origin}/`, socketNamespaceUrl: `${origin}/chat`, socketPath: '/socket.io' },
      messageCreator: { uid: 'user', name: 'User' }, onDiagnostic: vi.fn(), onUnhandledError: vi.fn(),
    }).create();
  });

  afterEach(async () => {
    cleanup();
    for (const response of heldResponses) if (!response.writableEnded) response.end();
    await client?.dispose({ deadlineAt: deadlineAt() });
    await new Promise<void>((resolve) => sockets.close(() => resolve()));
    http.closeAllConnections();
    if (http.listening) await new Promise<void>((resolve) => http.close(() => resolve()));
  });

  const load = (id: string) => client.loadConversation({ conversationId: id, deadlineAt: deadlineAt() });
  async function seed() {
    expect((await load('42')).ok).toBe(true);
    client.setComposerDraft({ conversationId: '42', text: 'Draft A', attachments: [{ uri: 's3://bucket/a.png', mimeType: 'image/png', name: 'A.png' }] });
    expect((await load('43')).ok).toBe(true);
  }
  function holdNext(id = 42) {
    const response = deferred<ServerResponse>(); hold = { id, response }; return response.promise;
  }
  async function select(id: number) {
    fireEvent.click(screen.getByRole('button', { name: 'History' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: `Conversation ${id}` }));
  }

  it('renders cached A in the actual compact workspace before server completion, then synchronizes', async () => {
    render(<ChatProvider client={client}><CompactChatWorkspace labels={chatUiLabels((key) => i18n.t(key))} /></ChatProvider>);
    await screen.findByText('History 42');
    act(() => { client.setComposerDraft({ conversationId: '42', text: 'Unsent draft' }); });
    await select(43);
    await screen.findByText('History 43');
    const pendingResponse = holdNext();
    await select(42);
    const response = await pendingResponse;
    await screen.findByText('History 42');
    expect(screen.getByText('Showing cached conversation. Syncing latest updates…')).toBeInTheDocument();
    expect(client.getCacheState()).toMatchObject({ source: 'memory', status: 'syncing' });
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    expect(screen.queryByRole('navigation', { name: 'Event navigation' })).not.toBeInTheDocument();
    freshA = true;
    await act(async () => { envelope(response, { messages: [dto(42), { ...dto(42, 'Fresh A'), msgId: 'fresh-a', createTimestamp: 1_800_000_000_001 }], events: [], cursor: null }); });

    await screen.findByText('Fresh A');
    await waitFor(() => expect(client.getCacheState().status).toBe('ready'));
    expect(screen.queryByText('Showing cached conversation. Syncing latest updates…')).not.toBeInTheDocument();
  // Real HTTP/Socket.IO and multiple workspace renders need a coverage-run budget.
  }, 15_000);

  it('dismisses a cache notice without changing authority and shows a new failure notice', async () => {
    await seed();
    render(<ChatProvider client={client}><ChatConversationView labels={chatUiLabels((key) => i18n.t(key))} getDeadlineAt={deadlineAt} /></ChatProvider>);
    const response = holdNext();
    let pending!: ReturnType<typeof load>;
    await act(async () => { pending = load('42'); await response; });
    const notice = screen.getByText('Showing cached conversation. Syncing latest updates…').closest('[role="alert"]')!;
    fireEvent.click(within(notice as HTMLElement).getByRole('button', { name: 'close' }));
    expect(screen.queryByText('Showing cached conversation. Syncing latest updates…')).not.toBeInTheDocument();
    expect(client.getCacheState().status).toBe('syncing');
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    await act(async () => { envelope(await response, null, 503); await pending; });
    expect(screen.getByText('Could not sync. Cached content may be out of date.')).toBeInTheDocument();
    expect(client.getCacheState().status).toBe('error');
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
  });

  it('keeps timeline event details available without the removed event navigation toolbar', async () => {
    expect((await load('42')).ok).toBe(true);
    render(<ChatProvider client={client}><ChatConversationView labels={chatUiLabels((key) => i18n.t(key))} getDeadlineAt={deadlineAt} eventDetailMode="modal" /></ChatProvider>);
    await act(async () => {
      sockets.of('/chat').emit('chat_event', {
        eventId: 'event-1', status: 'success', eventScene: 'Chain', conversationId: 42,
        createTimestamp: 1_800_000_000_001, exception: null, content: 'Completed fixture event',
      });
    });
    const trigger = await screen.findByRole('button', { name: /Open event details:/ });
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute('aria-pressed', 'true');
    expect(screen.queryByRole('navigation', { name: 'Event navigation' })).not.toBeInTheDocument();
    expect(await screen.findByRole('dialog', { name: 'Event details' })).toBeInTheDocument();
  });

  it('keeps cached history read-only after network failure, restores drafts and blocks live commands', async () => {
    await seed();
    failure = 503;
    const result = await load('42');
    expect(result.ok).toBe(false);
    expect(client.getCacheState()).toMatchObject({ source: 'memory', status: 'error' });
    expect(client.getSnapshot()?.timeline[0]?.id).toBe('message-42');
    expect(client.getComposerDraft('42')).toMatchObject({ text: 'Draft A', attachments: [{ uri: 's3://bucket/a.png' }] });
    expect((await client.sendText({ conversationId: '42', text: 'must not send', deadlineAt: deadlineAt() })).ok).toBe(false);
    expect((await client.interrupt({ conversationId: '42', deadlineAt: deadlineAt() })).ok).toBe(false);
    expect((await client.answerInteraction({ conversationId: '42', answer: { requestId: 'old-request', revision: '1', action: 'submit', answers: {} }, deadlineAt: deadlineAt() })).ok).toBe(false);
    expect(posts).toBe(0);
    failure = 0;
    expect((await load('42')).ok).toBe(true);
    expect(client.getCacheState().status).toBe('ready');
  });

  it.each([401, 403, 404])('invalidates cached content when synchronization returns %s', async (status) => {
    await seed(); failure = status;
    expect((await load('42')).ok).toBe(false);
    expect(client.getSnapshot()?.timeline ?? []).toHaveLength(0);
    expect(client.getComposerDraft('42').text).toBe('');
    failure = 0;
    const response = holdNext();
    const pending = load('42');
    const held = await response;
    expect(client.getCacheState().source).toBe('none');
    envelope(held, { messages: [dto(42)], events: [], cursor: null });
    expect((await pending).ok).toBe(true);
  });

  it('does not allow a late A response to replace C or a new client to inherit the old cache', async () => {
    await seed();
    const response = holdNext(); const pending = load('42'); const held = await response;
    expect((await load('44')).ok).toBe(true);
    envelope(held, { messages: [dto(42, 'Late A')], events: [], cursor: null });
    await pending;
    expect(client.getSnapshot()?.conversation.id).toBe('44');
    const replacement = createClientChatFactory({
      descriptor: { leaseId: 'new-identity', employeeId: 43, robotName: 'Other Robot', httpBaseUrl: `${origin}/`, socketNamespaceUrl: `${origin}/chat`, socketPath: '/socket.io' },
      messageCreator: { uid: 'other', name: 'Other' }, onDiagnostic: vi.fn(), onUnhandledError: vi.fn(),
    }).create();
    try {
      expect(replacement.getCacheState().source).toBe('none');
      expect(replacement.getSnapshot()).toBeNull();
      expect(replacement.getComposerDraft('42').text).toBe('');
    } finally { await replacement.dispose({ deadlineAt: deadlineAt() }); }
  });
});
