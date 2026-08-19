import { invoke } from '@tauri-apps/api/core';
import {
  createTFRobotChatGateway,
  isChatLifecycleOperable,
  type ChatUpdate,
  type TFRobotSocket,
  type TFRobotSocketAnyListener,
  type TFRobotSocketFactoryInput,
  type TFRobotSocketListener,
} from '@turingfocus/chat-kit/headless';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  CURRENT_SERVER_PROFILE,
  createTauriChatFetch,
} from '@/components/Chat/chatBridge';

const deadlineAt = (): number => Date.now() + 10_000;

class EmptyAckSocket implements TFRobotSocket {
  connected = false;
  readonly #getAuth: TFRobotSocketFactoryInput['getAuth'];
  readonly #listeners = new Map<string, Set<TFRobotSocketListener>>();
  readonly #anyListeners = new Set<TFRobotSocketAnyListener>();

  constructor(getAuth: TFRobotSocketFactoryInput['getAuth']) {
    this.#getAuth = getAuth;
  }

  connect(): void {
    void this.#getAuth().then(() => {
      this.connected = true;
      this.trigger('connect');
    });
  }

  disconnect(): void {
    this.connected = false;
  }

  emit(eventName: string, ...arguments_: unknown[]): void {
    if (eventName !== 'join_conversation') return;
    const acknowledgement = arguments_[1];
    if (typeof acknowledgement === 'function') {
      // Current TFRobotServer invokes the Socket.IO callback without an ACK payload.
      (acknowledgement as () => void)();
    }
  }

  on(eventName: string, listener: TFRobotSocketListener): void {
    const listeners = this.#listeners.get(eventName) ?? new Set<TFRobotSocketListener>();
    listeners.add(listener);
    this.#listeners.set(eventName, listeners);
  }

  off(eventName: string, listener: TFRobotSocketListener): void {
    this.#listeners.get(eventName)?.delete(listener);
  }

  onAny(listener: TFRobotSocketAnyListener): void {
    this.#anyListeners.add(listener);
  }

  offAny(listener: TFRobotSocketAnyListener): void {
    this.#anyListeners.delete(listener);
  }

  trigger(eventName: string, payload?: unknown): void {
    for (const listener of this.#listeners.get(eventName) ?? []) listener(payload);
    for (const listener of this.#anyListeners) listener(eventName, payload);
  }
}

interface ChatHttpRequest {
  body?: string;
  leaseId: string;
  method: string;
  url: string;
}

function tauriResponse(data: unknown): {
  status: number;
  body: string;
  contentType: string;
} {
  return {
    status: 200,
    body: JSON.stringify({ code: 200, message: 'Success', data }),
    contentType: 'application/json',
  };
}

describe('Chat Kit 0.7.0 current-server contract', () => {
  const history = [{
    msgId: 'message-existing',
    content: 'existing',
    additionalKwargs: {},
    attachments: null,
    createTimestamp: 1_786_425_600_000,
    creator: { uid: 'user', name: 'User', avatar: null },
    conversationId: 42,
    role: 'user',
    msgType: 'text',
  }];
  let interruptBody: unknown;

  beforeEach(() => {
    history.splice(1);
    interruptBody = undefined;
    vi.clearAllMocks();
    vi.mocked(invoke).mockImplementation(async (command, rawArguments) => {
      if (command !== 'chat_http_request') {
        throw new Error(`Unexpected Tauri command: ${command}`);
      }
      const request = rawArguments as unknown as ChatHttpRequest;
      const path = new URL(request.url).pathname;
      if (request.method === 'GET' && path.endsWith('/messages')) {
        return tauriResponse({ messages: history, events: [], cursor: null });
      }
      if (request.method === 'GET' && path.endsWith('/status')) {
        return tauriResponse({ working: false, task_id: null });
      }
      if (request.method === 'POST' && path.endsWith('/messages')) {
        return tauriResponse({ task_id: 'run-from-current-server' });
      }
      if (request.method === 'POST' && path.endsWith('/interrupt')) {
        interruptBody = JSON.parse(request.body ?? 'null');
        return tauriResponse({ task_id: 'cancel-from-current-server' });
      }
      throw new Error(`Unexpected request: ${request.method} ${path}`);
    });
  });

  it('keeps the verified profile strict when join_conversation returns an empty ACK', async () => {
    const gateway = createTFRobotChatGateway({
      baseUrl: 'https://robot.example/proxy/',
      fetch: createTauriChatFetch('lease-contract'),
      messageCreatorProvider: () => ({ uid: 'user', name: 'User' }),
      sessionProvider: {
        getSession: () => ({ kind: 'bearer', token: 'fixture-token' }),
      },
      socketFactory: (input) => new EmptyAckSocket(input.getAuth),
    });

    const subscribed = await gateway.subscribe(
      { conversationId: '42', deadlineAt: deadlineAt() },
      { next: vi.fn() },
    );

    expect(subscribed).toMatchObject({
      ok: false,
      error: { code: 'validation' },
    });
    await gateway.dispose({ deadlineAt: deadlineAt() });
  });

  it('preflights empty ACK, preserves run IDs, interrupts, and rebases after reconnect', async () => {
    const socketHolder: { current?: EmptyAckSocket } = {};
    const updates: ChatUpdate[] = [];
    const gateway = createTFRobotChatGateway({
      baseUrl: 'https://robot.example/proxy/',
      fetch: createTauriChatFetch('lease-contract'),
      messageCreatorProvider: () => ({ uid: 'user', name: 'User' }),
      serverProfile: CURRENT_SERVER_PROFILE,
      sessionProvider: {
        getSession: () => ({ kind: 'bearer', token: 'fixture-token' }),
      },
      socketFactory: (input) => {
        const socket = new EmptyAckSocket(input.getAuth);
        socketHolder.current = socket;
        return socket;
      },
    });

    const subscribed = await gateway.subscribe(
      { conversationId: '42', deadlineAt: deadlineAt() },
      { next: (update) => updates.push(update) },
    );
    expect(subscribed).toMatchObject({ ok: true });
    const degraded = updates.find(
      (update) => update.kind === 'lifecycle.changed'
        && update.lifecycle.status === 'degraded',
    );
    expect(degraded).toMatchObject({
      lifecycle: {
        status: 'degraded',
        recovery: {
          assurance: 'best-effort',
          complete: false,
          reason: 'initial-rest-preflight',
          source: 'rest-rebase',
        },
      },
    });
    if (degraded?.kind !== 'lifecycle.changed') {
      throw new Error('Expected a degraded lifecycle update');
    }
    expect(isChatLifecycleOperable(degraded.lifecycle)).toBe(true);

    const sent = await gateway.sendText({
      conversationId: '42',
      deadlineAt: deadlineAt(),
      text: 'send through current Server',
    });
    expect(sent).toEqual({
      ok: true,
      value: { runId: 'run-from-current-server' },
    });

    const interrupted = await gateway.interrupt({
      conversationId: '42',
      deadlineAt: deadlineAt(),
      runId: 'run-from-current-server',
    });
    expect(interrupted).toEqual({
      ok: true,
      value: {
        cancellationId: 'cancel-from-current-server',
        interruptedRunId: 'run-from-current-server',
      },
    });
    expect(interruptBody).toEqual({ taskId: 'run-from-current-server' });

    history.push({
      msgId: 'message-missed-while-offline',
      content: 'recovered by REST rebase',
      additionalKwargs: {},
      attachments: null,
      createTimestamp: 1_786_425_600_100,
      creator: { uid: 'robot', name: 'Robot', avatar: null },
      conversationId: 42,
      role: 'assistant',
      msgType: 'text',
    });
    const socket = socketHolder.current;
    if (socket === undefined) throw new Error('Expected a Socket transport');
    socket.connected = false;
    socket.trigger('disconnect', 'transport close');
    socket.connect();

    await vi.waitFor(() => {
      expect(updates.some(
        (update) => update.kind === 'timeline.upsert'
          && update.item.id === 'message-missed-while-offline',
      )).toBe(true);
      expect(updates.some(
        (update) => update.kind === 'lifecycle.changed'
          && update.lifecycle.status === 'degraded'
          && update.lifecycle.recovery?.complete === false
          && update.lifecycle.recovery.assurance === 'best-effort'
          && update.lifecycle.recovery.source === 'rest-rebase',
      )).toBe(true);
    });

    if (subscribed.ok) await subscribed.value.dispose({ deadlineAt: deadlineAt() });
    await gateway.dispose({ deadlineAt: deadlineAt() });
  });
});
