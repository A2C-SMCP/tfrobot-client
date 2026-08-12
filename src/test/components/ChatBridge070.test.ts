import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  CURRENT_SERVER_PROFILE,
  chatUiLabels,
  createClientChatFactory,
  type ChatSessionDescriptor,
} from '@/components/Chat/chatBridge';

const chatKitMock = vi.hoisted(() => ({
  createFactory: vi.fn((_options: unknown) => ({
    create: vi.fn(),
    getDisposeOptions: vi.fn(),
  })),
}));

vi.mock('@turingfocus/chat-kit', () => ({
  createTFRobotChatClientFactory: chatKitMock.createFactory,
}));

describe('Chat Kit 0.7.0 bridge configuration', () => {
  const descriptor: ChatSessionDescriptor = {
    leaseId: 'lease-bridge-contract',
    employeeId: 42,
    robotName: 'Robot A',
    httpBaseUrl: 'https://robot.example/proxy/',
    socketNamespaceUrl: 'https://robot.example/chat',
    socketPath: '/socket.io',
  };

  beforeEach(() => vi.clearAllMocks());

  it('passes the bounded current-server profile to Chat Kit', () => {
    createClientChatFactory({
      descriptor,
      messageCreator: { uid: 'account-1', name: 'Alice' },
      onDiagnostic: vi.fn(),
      onUnhandledError: vi.fn(),
    });

    expect(chatKitMock.createFactory).toHaveBeenCalledWith(expect.objectContaining({
      serverProfile: CURRENT_SERVER_PROFILE,
    }));
  });

  it('maps degraded lifecycle and conversation-management labels', () => {
    const labels = chatUiLabels((key) => key);

    expect(labels.lifecycleStatus).toEqual({
      degraded: 'chat.workspace.lifecycleDegraded',
    });
    expect(labels.renameConversation).toBe('chat.workspace.renameConversation');
    expect(labels.renameConversationConfirm).toBe('chat.workspace.renameConversationConfirm');
    expect(labels.deleteConversation).toBe('chat.workspace.deleteConversation');
    expect(labels.deleteConversationCancel).toBe('chat.workspace.deleteConversationCancel');
    expect(labels.deleteConversationConfirm).toBe('chat.workspace.deleteConversationConfirm');
    expect(labels.deleteConversationPrompt).toBe('chat.workspace.deleteConversationPrompt');
  });
});
