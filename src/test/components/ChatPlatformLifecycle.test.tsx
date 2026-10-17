import { createServer } from 'node:http';
import { Server } from 'socket.io';
import { invoke } from '@tauri-apps/api/core';
import { expect, it, vi } from 'vitest';
import { Chat } from '@/components/Chat';
import { managerContextScope, useManagerStore } from '@/stores/managerStore';
import i18n from '@/i18n';
import { fireEvent, render, screen, waitFor } from '../helpers/render';

it('keeps a real Kit draft on preference failure and disposes clients only on committed platform switches', async () => {
  await i18n.changeLanguage('en');
  const previous = useManagerStore.getState();
  const context = {
    revision: 1, authState: 'authenticated' as const, environment: 'staging' as const,
    contextKey: { environment: 'staging' as const, accountId: 'fixture', organizationId: 'fixture' },
    user: { id: 'fixture', nickname: 'Fixture', email: '', phone: '' },
    account: { id: 'fixture', name: 'Fixture', nickname: 'Fixture', avatar: '', employeeNo: '' },
    organization: { id: 'fixture', name: 'Fixture', organizationType: 'company' }, permissions: [],
  };
  const scope = managerContextScope(context)!;
  useManagerStore.setState({ context, employeeResources: { [scope]: {
    scope, contextKey: context.contextKey, revision: 1,
    employees: [{ id: 42, name: 'Robot', robotAccountId: 'fixture', status: 'running', templateType: 'tfrserver' }],
    loading: false, error: null, paymentRequired: null, lastFetchAt: Date.now(),
    selectedEmployeeId: null, connectingEmployeeId: null,
  } } });
  const http = createServer((request, response) => {
    const path = new URL(request.url!, 'http://fixture').pathname;
    const id = path.includes('/43/') ? 43 : 42;
    const data = path.endsWith('/platforms') ? [{ platformId: 1, name: 'A2A' }, { platformId: 2, name: 'Workspace' }]
      : path.endsWith('/status') ? { working: false, task_id: null }
        : path.endsWith('/messages') ? { messages: [{ msgId: `message-${id}`, conversationId: id,
          content: `History ${id}`, additionalKwargs: {}, attachments: null, createTimestamp: 1_800_000_000_000,
          creator: { uid: 'agent', name: 'Agent', avatar: null }, role: 'assistant', msgType: 'text' }], events: [], cursor: null }
          : { conversations: [], cursor: null };
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ code: 200, message: 'Success', data }));
  });
  const sockets = new Server(http, { transports: ['websocket'] });
  let connects = 0;
  let disconnects = 0;
  sockets.of('/chat').on('connection', (socket) => {
    connects++;
    socket.on('disconnect', () => { disconnects++; });
    socket.on('join_conversation', (_data, ack) => ack());
  });
  await new Promise<void>((resolve) => http.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${(http.address() as { port: number }).port}`;
  let failSave = false;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const parameters = args as { platformId?: number; url?: string; method?: string };
    if (command === 'chat_get_recent_robot') return 42;
    if (command === 'chat_open_session') return { leaseId: 'platforms', employeeId: 42, robotName: 'Robot',
      httpBaseUrl: `${origin}/`, socketNamespaceUrl: `${origin}/chat`, socketPath: '/socket.io' };
    if (command === 'chat_platform_preference') {
      if (parameters.platformId !== undefined && failSave) throw new Error('Disk unavailable');
      return parameters.platformId ?? 1;
    }
    if (command === 'chat_get_recent_conversation') return parameters.platformId === 2 ? '43' : '42';
    if (command === 'chat_get_session_token') return { token: 'fixture', expiresAt: Date.now() + 100_000 };
    if (command === 'chat_http_request') {
      const response = await fetch(parameters.url!, { method: parameters.method });
      return { status: response.status, body: await response.text(), contentType: 'application/json' };
    }
    if (['chat_remember_robot', 'chat_remember_conversation', 'chat_close_session'].includes(command)) return;
    throw new Error(`Unexpected IPC ${command}`);
  });
  const view = render(<Chat />);
  const choose = async (label: string) => {
    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Conversation platform' }));
    fireEvent.click(await screen.findByText(label));
  };
  try {
    await screen.findByText('History 42');
    fireEvent.change(screen.getByRole('textbox', { name: 'Message' }), { target: { value: 'Keep my platform A draft' } });
    failSave = true;
    await choose('Workspace');
    await screen.findByText('Could not save the platform selection. Your current conversation is preserved. Please retry.');
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('Keep my platform A draft');
    expect(connects).toBe(1);
    expect(disconnects).toBe(0);
    failSave = false;
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await screen.findByText('History 43');
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('');
    await waitFor(() => expect(disconnects).toBe(1));
    await choose('A2A');
    await screen.findByText('History 42');
    expect(screen.getByRole('textbox', { name: 'Message' })).toHaveValue('');
    await waitFor(() => expect(disconnects).toBe(2));
    expect(connects).toBe(3);
  } finally {
    view.unmount();
    http.closeAllConnections();
    await new Promise<void>((resolve) => sockets.close(() => resolve()));
    useManagerStore.setState(previous);
  }
}, 30_000);
