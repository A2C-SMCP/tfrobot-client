import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { invoke } from '@tauri-apps/api/core';
import { StrictMode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Chat } from '@/components/Chat';
import {
  chatRobotDisabledReason,
  orderChatRobots,
  preferredChatRobotId,
} from '@/components/Chat/availability';
import {
  createClientChatFactory,
  createTauriChatFetch,
  type ChatSessionDescriptor,
} from '@/components/Chat/chatBridge';
import type {
  DigitalEmployeeBrief,
  ManagerContextSnapshot,
  ManagerEmployeeResource,
} from '@/stores/managerStore';

const chatKitMock = vi.hoisted(() => ({
  createFactory: vi.fn((_options: unknown) => ({ create: vi.fn(), getDisposeOptions: vi.fn() })),
  useConversationWorkspace: vi.fn(),
}));

vi.mock('@turingfocus/chat-kit', () => ({
  createTFRobotChatClientFactory: chatKitMock.createFactory,
  OwnedChatProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ChatUiShell: () => <div>Managed Chat Workspace</div>,
  ChatConversationView: () => null,
  useConversationWorkspace: chatKitMock.useConversationWorkspace,
}));

const employee = (overrides: Partial<DigitalEmployeeBrief> = {}): DigitalEmployeeBrief => ({
  id: 42,
  name: 'Robot A',
  robotAccountId: 'robot-account-42',
  status: 'running',
  templateType: 'tfrserver',
  ...overrides,
});

const authenticatedContext: ManagerContextSnapshot = {
  revision: 7,
  authState: 'authenticated',
  environment: 'staging',
  contextKey: {
    environment: 'staging',
    accountId: 'account-1',
    organizationId: 'organization-1',
  },
  user: { id: 'user-1', nickname: 'User', email: '', phone: '' },
  account: { id: 'account-1', name: 'Account', nickname: 'Alice', avatar: '', employeeNo: '' },
  organization: { id: 'organization-1', name: 'Organization', organizationType: 'company' },
  permissions: [],
};

const managerStoreMock = vi.hoisted(() => ({
  context: null as ManagerContextSnapshot | null,
  employeeResources: {} as Record<string, unknown>,
  identityError: null,
  fetchEmployees: vi.fn().mockResolvedValue(undefined),
  fetchEmployeesIfStale: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@/stores/managerStore', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/stores/managerStore')>();
  return {
    ...actual,
    useManagerStore: () => ({
      ...managerStoreMock,
      context: managerStoreMock.context,
    }),
  };
});

function setAuthenticatedEmployees(employees: DigitalEmployeeBrief[]): void {
  managerStoreMock.context = authenticatedContext;
  const scope = JSON.stringify(['staging', 'account-1', 'organization-1', 7]);
  managerStoreMock.employeeResources = {
    [scope]: {
      scope,
      contextKey: authenticatedContext.contextKey,
      revision: 7,
      employees,
      loading: false,
      error: null,
      paymentRequired: null,
      lastFetchAt: Date.now(),
      selectedEmployeeId: null,
      connectingEmployeeId: null,
    },
  };
}

function currentEmployeeResource(): ManagerEmployeeResource {
  return Object.values(managerStoreMock.employeeResources)[0] as ManagerEmployeeResource;
}

describe('Chat', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    managerStoreMock.context = {
      revision: 0,
      authState: 'signed_out',
      environment: null,
      contextKey: null,
      user: null,
      account: null,
      organization: null,
      permissions: [],
    };
    managerStoreMock.employeeResources = {};
    chatKitMock.useConversationWorkspace.mockReturnValue({
      controller: {},
      ready: true,
      snapshot: {
        conversations: [],
        creating: false,
        listStatus: 'idle',
        selectionStatus: 'ready',
      },
      createConversation: vi.fn(),
      loadMore: vi.fn(),
      refresh: vi.fn(),
      selectConversation: vi.fn(),
    });
  });

  it('requires a Manager login before showing Robot chat controls', () => {
    render(<Chat />);
    expect(screen.getByText('Sign in to Manager to chat with an available Robot.')).toBeInTheDocument();
    expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
  });

  it('classifies unavailable Robots without guessing when template metadata is absent', () => {
    expect(chatRobotDisabledReason(employee())).toBeNull();
    expect(chatRobotDisabledReason(employee({ templateType: undefined }))).toBeNull();
    expect(chatRobotDisabledReason(employee({ status: 'stopped' }))).toBe('notRunning');
    expect(chatRobotDisabledReason(employee({ robotAccountId: undefined }))).toBe('missingAccount');
    expect(chatRobotDisabledReason(employee({ templateType: 'tfropenclaw' }))).toBe('incompatible');
  });

  it('presents organization context and automatically opens the available Robot', async () => {
    setAuthenticatedEmployees([
      employee(),
      employee({ id: 43, name: 'Robot B', status: 'stopped' }),
    ]);
    const descriptor: ChatSessionDescriptor = {
      leaseId: 'lease-auto',
      employeeId: 42,
      robotName: 'Robot A',
      httpBaseUrl: 'https://robot.example/proxy/',
      socketNamespaceUrl: 'https://robot.example/chat',
      socketPath: '/socket.io',
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_get_recent_robot') return null;
      if (command === 'chat_open_session') return descriptor;
      return undefined;
    });

    render(<Chat />);

    expect(screen.getByRole('heading', { name: 'Chat' })).toBeInTheDocument();
    expect(screen.getByText('Organization: Organization')).toBeInTheDocument();
    expect(screen.getByText('1 available')).toBeInTheDocument();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', {
      employeeId: 42,
      selectionRevision: expect.any(Number),
    }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_remember_robot', {
      leaseId: 'lease-auto',
    }));
    expect(await screen.findByText('Managed Chat Workspace')).toBeInTheDocument();
  });

  it('assigns a unique revision to each StrictMode session-open attempt', async () => {
    setAuthenticatedEmployees([employee()]);
    let leaseNumber = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_get_recent_robot') return null;
      if (command === 'chat_open_session') {
        leaseNumber += 1;
        return {
          leaseId: `lease-strict-${leaseNumber}`,
          employeeId: 42,
          robotName: 'Robot A',
          httpBaseUrl: 'https://robot.example/proxy/',
          socketNamespaceUrl: 'https://robot.example/chat',
          socketPath: '/socket.io',
        } satisfies ChatSessionDescriptor;
      }
      return undefined;
    });

    render(<StrictMode><Chat /></StrictMode>);

    await waitFor(() => {
      const revisions = vi.mocked(invoke).mock.calls
        .filter(([command]) => command === 'chat_open_session')
        .map(([, args]) => (args as { selectionRevision: number }).selectionRevision);
      expect(revisions.length).toBeGreaterThanOrEqual(2);
      expect(new Set(revisions).size).toBe(revisions.length);
    });
  });

  it('opens an in-memory lease for the automatically selected Robot and closes it on unmount', async () => {
    setAuthenticatedEmployees([employee()]);
    const descriptor: ChatSessionDescriptor = {
      leaseId: 'lease-1',
      employeeId: 42,
      robotName: 'Robot A',
      httpBaseUrl: 'https://robot.example/proxy/',
      socketNamespaceUrl: 'https://robot.example/chat',
      socketPath: '/socket.io',
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_get_recent_robot') return null;
      if (command === 'chat_open_session') return descriptor;
      return undefined;
    });

    const view = render(<Chat />);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', {
      employeeId: 42,
      selectionRevision: expect.any(Number),
    }));
    expect(await screen.findByText('Managed Chat Workspace')).toBeInTheDocument();

    view.unmount();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_close_session', {
      leaseId: 'lease-1',
    }));
  });

  it('keeps the active chat lease mounted while the Robot list refreshes', async () => {
    setAuthenticatedEmployees([employee()]);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_get_recent_robot') return null;
      if (command === 'chat_open_session') {
        return {
          leaseId: 'lease-refresh',
          employeeId: 42,
          robotName: 'Robot A',
          httpBaseUrl: 'https://robot.example/proxy/',
          socketNamespaceUrl: 'https://robot.example/chat',
          socketPath: '/socket.io',
        } satisfies ChatSessionDescriptor;
      }
      return undefined;
    });

    const view = render(<Chat />);
    expect(await screen.findByText('Managed Chat Workspace')).toBeInTheDocument();

    currentEmployeeResource().loading = true;
    view.rerender(<Chat />);

    expect(screen.getByText('Managed Chat Workspace')).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith('chat_close_session', {
      leaseId: 'lease-refresh',
    });
  });

  it('does not open the old Robot ID against a newly switched Manager context', async () => {
    setAuthenticatedEmployees([employee()]);
    let recentRequestCount = 0;
    let resolveNewContextPreference: ((employeeId: number | null) => void) | undefined;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'chat_get_recent_robot') {
        recentRequestCount += 1;
        if (recentRequestCount === 1) return null;
        return new Promise<number | null>((resolve) => {
          resolveNewContextPreference = resolve;
        });
      }
      if (command === 'chat_open_session') {
        const employeeId = (args as { employeeId: number }).employeeId;
        return {
          leaseId: `lease-${employeeId}-${recentRequestCount}`,
          employeeId,
          robotName: 'Robot',
          httpBaseUrl: 'https://robot.example/proxy/',
          socketNamespaceUrl: 'https://robot.example/chat',
          socketPath: '/socket.io',
        } satisfies ChatSessionDescriptor;
      }
      return undefined;
    });

    const view = render(<Chat />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', {
      employeeId: 42,
      selectionRevision: expect.any(Number),
    }));

    const switchedContext: ManagerContextSnapshot = {
      ...authenticatedContext,
      revision: 8,
      contextKey: {
        environment: 'staging',
        accountId: 'account-2',
        organizationId: 'organization-1',
      },
      account: {
        ...authenticatedContext.account!,
        id: 'account-2',
        name: 'Account 2',
      },
    };
    const switchedScope = JSON.stringify(['staging', 'account-2', 'organization-1', 8]);
    managerStoreMock.context = switchedContext;
    managerStoreMock.employeeResources = {
      [switchedScope]: {
        scope: switchedScope,
        contextKey: switchedContext.contextKey,
        revision: 8,
        employees: [employee({ id: 43, name: 'Robot B', robotAccountId: 'robot-account-43' })],
        loading: false,
        error: null,
        paymentRequired: null,
        lastFetchAt: Date.now(),
        selectedEmployeeId: null,
        connectingEmployeeId: null,
      },
    };
    view.rerender(<Chat />);
    await waitFor(() => expect(recentRequestCount).toBe(2));

    expect(vi.mocked(invoke).mock.calls
      .filter(([command]) => command === 'chat_open_session')).toHaveLength(1);

    await act(async () => {
      resolveNewContextPreference?.(null);
    });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', {
      employeeId: 43,
      selectionRevision: expect.any(Number),
    }));
  });

  it('uses the latest successfully opened Robot for a later fallback in the same run', async () => {
    const robotA = employee({ id: 42, name: 'Robot A' });
    const robotB = employee({ id: 43, name: 'Robot B', robotAccountId: 'robot-account-43' });
    setAuthenticatedEmployees([robotA, robotB]);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'chat_get_recent_robot') return 42;
      if (command === 'chat_open_session') {
        const employeeId = (args as { employeeId: number }).employeeId;
        return {
          leaseId: `lease-${employeeId}`,
          employeeId,
          robotName: `Robot ${employeeId}`,
          httpBaseUrl: 'https://robot.example/proxy/',
          socketNamespaceUrl: 'https://robot.example/chat',
          socketPath: '/socket.io',
        } satisfies ChatSessionDescriptor;
      }
      return undefined;
    });

    const view = render(<Chat />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', {
      employeeId: 42,
      selectionRevision: expect.any(Number),
    }));
    fireEvent.mouseDown(screen.getByRole('combobox'));
    const robotBOptions = await screen.findAllByText('Robot B');
    fireEvent.click(robotBOptions[robotBOptions.length - 1]);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', {
      employeeId: 43,
      selectionRevision: expect.any(Number),
    }));

    currentEmployeeResource().employees = [];
    view.rerender(<Chat />);
    await waitFor(() => expect(screen.getByText('Select a Robot to start')).toBeInTheDocument());

    currentEmployeeResource().employees = [robotA, robotB];
    view.rerender(<Chat />);
    await waitFor(() => {
      const openedEmployees = vi.mocked(invoke).mock.calls
        .filter(([command]) => command === 'chat_open_session')
        .map(([, args]) => (args as { employeeId: number }).employeeId);
      expect(openedEmployees).toEqual([42, 43, 43]);
    });
  });

  it('prefers the recent available Robot over the stable name order', async () => {
    setAuthenticatedEmployees([
      employee({ id: 42, name: 'Robot Z' }),
      employee({ id: 43, name: 'Robot A', robotAccountId: 'robot-account-43' }),
    ]);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'chat_get_recent_robot') return 42;
      if (command === 'chat_open_session') {
        const employeeId = (args as { employeeId: number }).employeeId;
        return {
          leaseId: `lease-${employeeId}`,
          employeeId,
          robotName: 'Robot',
          httpBaseUrl: 'https://robot.example/proxy/',
          socketNamespaceUrl: 'https://robot.example/chat',
          socketPath: '/socket.io',
        } satisfies ChatSessionDescriptor;
      }
      return undefined;
    });

    render(<Chat />);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', {
      employeeId: 42,
      selectionRevision: expect.any(Number),
    }));
  });

  it('uses stable name and ID ordering when the recent Robot is unavailable', () => {
    const ordered = orderChatRobots([
      employee({ id: 44, name: 'Robot 10' }),
      employee({ id: 43, name: 'Robot 2' }),
      employee({ id: 42, name: 'Robot 2' }),
      employee({ id: 41, name: 'Robot 1', status: 'stopped' }),
    ], 'en');

    expect(ordered.map((candidate) => candidate.id)).toEqual([41, 42, 43, 44]);
    expect(preferredChatRobotId(ordered, null, 99)).toBe(42);
    expect(preferredChatRobotId(ordered, 43, 42)).toBe(43);
    expect(preferredChatRobotId([
      employee({ id: 45, status: 'stopped' }),
    ], null, null)).toBeNull();
  });

  it('shows a session error without silently trying the next available Robot', async () => {
    setAuthenticatedEmployees([
      employee({ id: 42, name: 'Robot A' }),
      employee({ id: 43, name: 'Robot B', robotAccountId: 'robot-account-43' }),
    ]);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_get_recent_robot') return null;
      if (command === 'chat_open_session') throw { kind: 'network_error', detail: 'offline' };
      return undefined;
    });

    render(<Chat />);

    expect(await screen.findByText('Unable to open the chat session')).toBeInTheDocument();
    const openCalls = vi.mocked(invoke).mock.calls
      .filter(([command]) => command === 'chat_open_session');
    expect(openCalls).toHaveLength(1);
    expect(openCalls[0]?.[1]).toEqual({
      employeeId: 42,
      selectionRevision: expect.any(Number),
    });
  });
});

describe('chat bridge', () => {
  const descriptor: ChatSessionDescriptor = {
    leaseId: 'lease-2',
    employeeId: 42,
    robotName: 'Robot A',
    httpBaseUrl: 'https://robot.example/proxy/',
    socketNamespaceUrl: 'https://robot.example/chat',
    socketPath: '/socket.io',
  };

  beforeEach(() => vi.clearAllMocks());

  it('sends only the URL, method and body through the narrow BFF adapter', async () => {
    vi.mocked(invoke).mockResolvedValue({
      status: 200,
      body: '{"code":200,"data":{}}',
      contentType: 'application/json',
    });
    const fetch = createTauriChatFetch('lease-2');
    const response = await fetch('https://robot.example/proxy/v1/chat/conversations', {
      method: 'POST',
      headers: { Authorization: 'Bearer must-not-cross-ipc' },
      body: '{"title":"New"}',
    });

    expect(response.status).toBe(200);
    expect(invoke).toHaveBeenCalledWith('chat_http_request', {
      leaseId: 'lease-2',
      url: 'https://robot.example/proxy/v1/chat/conversations',
      method: 'POST',
      body: '{"title":"New"}',
    });
    expect(JSON.stringify(vi.mocked(invoke).mock.calls)).not.toContain('must-not-cross-ipc');
  });

  it('obtains short credentials on demand and invalidates them after rejection', async () => {
    vi.mocked(invoke).mockResolvedValue({ token: 'short-token', expiresAt: Date.now() + 300_000 });
    createClientChatFactory({
      descriptor,
      messageCreator: { uid: 'account-1', name: 'Alice' },
      onDiagnostic: vi.fn(),
      onUnhandledError: vi.fn(),
    });
    const options = chatKitMock.createFactory.mock.calls[0]?.[0] as {
      sessionProvider: {
        getSession: (request: { purpose: string; operation: string }) => Promise<unknown>;
        onSessionInvalid: (invalidation: unknown) => Promise<void>;
      };
    };

    await expect(options.sessionProvider.getSession({ purpose: 'request', operation: 'read' }))
      .resolves.toEqual({ kind: 'bearer', token: 'short-token' });
    await act(async () => {
      await options.sessionProvider.onSessionInvalid({
        reason: 'expired',
        error: { code: 'authentication', message: 'expired', retryable: true },
      });
    });
    expect(invoke).toHaveBeenCalledWith('chat_invalidate_session', { leaseId: 'lease-2' });
  });

});
