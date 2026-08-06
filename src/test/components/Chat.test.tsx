import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Chat } from '@/components/Chat';
import { chatRobotDisabledReason } from '@/components/Chat/availability';
import {
  createClientChatFactory,
  createTauriChatFetch,
  type ChatSessionDescriptor,
} from '@/components/Chat/chatBridge';
import type { DigitalEmployeeBrief, ManagerContextSnapshot } from '@/stores/managerStore';

const chatKitMock = vi.hoisted(() => ({
  createFactory: vi.fn((_options: unknown) => ({ create: vi.fn(), getDisposeOptions: vi.fn() })),
}));

vi.mock('@turingfocus/chat-kit', () => ({
  createTFRobotChatClientFactory: chatKitMock.createFactory,
  OwnedChatProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  ChatWorkspace: () => <div>Managed Chat Workspace</div>,
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

  it('opens an in-memory lease for the selected Robot and closes it on unmount', async () => {
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
      if (command === 'chat_open_session') return descriptor;
      return undefined;
    });

    const view = render(<Chat />);
    fireEvent.mouseDown(screen.getByRole('combobox'));
    const robotOptions = await screen.findAllByText('Robot A');
    fireEvent.click(robotOptions[robotOptions.length - 1]);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_open_session', { employeeId: 42 }));
    expect(await screen.findByText('Managed Chat Workspace')).toBeInTheDocument();

    view.unmount();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('chat_close_session', {
      leaseId: 'lease-1',
    }));
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
