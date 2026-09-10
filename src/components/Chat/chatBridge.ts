import { invoke } from '@tauri-apps/api/core';
import { createTauriChatFetch } from './chatTransport';
export { createTauriChatFetch } from './chatTransport';
import {
  createTFRobotChatClientFactory,
  type ChatClientFactory,
  type ChatError,
  type ChatUiLabelOverrides,
  type SessionInvalidation,
  type SessionProvider,
  type TFRobotSession,
} from '@turingfocus/chat-kit';

export interface ChatSessionDescriptor {
  leaseId: string;
  employeeId: number;
  robotName: string;
  httpBaseUrl: string;
  socketNamespaceUrl: string;
  socketPath: string;
}

interface ChatSessionCredential {
  token: string;
  expiresAt: number;
}

export interface ChatMessageCreator {
  uid: string;
  name: string;
}

const REQUEST_TIMEOUT_MS = 15_000;

export const CURRENT_SERVER_PROFILE = Object.freeze({
  kind: 'current-server' as const,
  rebase: Object.freeze({
    deadlineMs: 10_000,
    maxItems: 500,
    maxPages: 10,
    pageSize: 50,
  }),
});

export const getChatDeadlineAt = (): number => Date.now() + REQUEST_TIMEOUT_MS;

interface CreateChatFactoryOptions {
  descriptor: ChatSessionDescriptor;
  messageCreator: ChatMessageCreator;
  onDiagnostic: (error: ChatError) => void;
  onUnhandledError: (error: unknown) => void;
}

export function createClientChatFactory({
  descriptor,
  messageCreator,
  onDiagnostic,
  onUnhandledError,
}: CreateChatFactoryOptions): ChatClientFactory {
  const sessionProvider: SessionProvider<TFRobotSession> = {
    async getSession() {
      const credential = await invoke<ChatSessionCredential>('chat_get_session_token', {
        leaseId: descriptor.leaseId,
      });
      return { kind: 'bearer', token: credential.token };
    },
    async onSessionInvalid(_invalidation: SessionInvalidation) {
      await invoke('chat_invalidate_session', { leaseId: descriptor.leaseId });
    },
  };

  return createTFRobotChatClientFactory({
    baseUrl: descriptor.httpBaseUrl,
    socketNamespaceUrl: descriptor.socketNamespaceUrl,
    socketPath: descriptor.socketPath,
    serverProfile: CURRENT_SERVER_PROFILE,
    sessionProvider,
    messageCreatorProvider: () => messageCreator,
    fetch: createTauriChatFetch(descriptor.leaseId),
    onDiagnostic,
    onUnhandledError,
    getDisposeOptions: () => ({ deadlineAt: getChatDeadlineAt() }),
  });
}

export function chatUiLabels(t: (key: string) => string): ChatUiLabelOverrides {
  const label = (key: string) => t(`chat.workspace.${key}`);
  return {
    cacheSyncing: label('cacheSyncing'),
    cacheStale: label('cacheStale'),
    cacheSyncFailed: label('cacheSyncFailed'),
    cacheAttachmentUnavailable: label('cacheAttachmentUnavailable'),
    cacheStorageFailed: label('cacheStorageFailed'),
    resource: {
      'screenshot': label('resource.screenshot'),
      'generatedFile': label('resource.generatedFile'),
      'fileTitle': label('resource.fileTitle'),
      'image': label('resource.image'),
      'audio': label('resource.audio'),
      'video': label('resource.video'),
      'loading': label('resource.loading'),
      'retry': label('resource.retry'),
      'open': label('resource.open'),
      'download': label('resource.download'),
      'opening': label('resource.opening'),
      'downloading': label('resource.downloading'),
      'unauthorized': label('resource.unauthorized'),
      'network': label('resource.network'),
      'not-found': label('resource.not-found'),
      'expired': label('resource.expired'),
      'unsupported': label('resource.unsupported'),
      'cancelled': label('resource.cancelled'),
      'unknown': label('resource.unknown'),
    },
    attach: label('attach'),
    pastedTextTitle: label('pastedTextTitle'),
    removeAttachment: label('removeAttachment'),
    removeLongText: label('removeLongText'),
    retryUpload: label('retryUpload'),
    askUserCancel: label('askUserCancel'),
    askUserChatAboutThis: label('askUserChatAboutThis'),
    askUserLabel: label('askUserLabel'),
    askUserRequired: label('askUserRequired'),
    askUserSubmit: label('askUserSubmit'),
    askUserSubmitting: label('askUserSubmitting'),
    askUserUnavailable: label('askUserUnavailable'),
    capabilityUnavailableDescription: label('capabilityUnavailableDescription'),
    capabilityUnavailableTitle: label('capabilityUnavailableTitle'),
    composerLabel: label('composerLabel'),
    composerPlaceholder: label('composerPlaceholder'),
    conversationListLabel: label('conversationListLabel'),
    historyConversations: label('historyConversations'),
    loadingHistoryConversations: label('loadingHistoryConversations'),
    newConversation: label('newConversation'),
    noHistoryConversations: label('noHistoryConversations'),
    createConversation: label('createConversation'),
    createConversationConfirm: label('createConversationConfirm'),
    createConversationTitleLabel: label('createConversationTitleLabel'),
    createConversationTitlePlaceholder: label('createConversationTitlePlaceholder'),
    deleteConversation: label('deleteConversation'),
    deleteConversationCancel: label('deleteConversationCancel'),
    deleteConversationConfirm: label('deleteConversationConfirm'),
    deleteConversationPrompt: label('deleteConversationPrompt'),
    disconnectedDescription: label('disconnectedDescription'),
    disconnectedTitle: label('disconnectedTitle'),
    emptyDescription: label('emptyDescription'),
    emptyTitle: label('emptyTitle'),
    errorDescription: label('errorDescription'),
    errorTitle: label('errorTitle'),
    loadingDescription: label('loadingDescription'),
    loadingTitle: label('loadingTitle'),
    emptyTimeline: label('emptyTimeline'),
    eventDetailClose: label('eventDetailClose'),
    eventDetailEmpty: label('eventDetailEmpty'),
    eventDetailModeAuto: label('eventDetailModeAuto'),
    eventDetailModeLabel: label('eventDetailModeLabel'),
    eventDetailModeModal: label('eventDetailModeModal'),
    eventDetailModeSplit: label('eventDetailModeSplit'),
    eventDetailSplitHandleLabel: label('eventDetailSplitHandleLabel'),
    eventDetailTitle: label('eventDetailTitle'),
    interrupt: label('interrupt'),
    interruptUnavailable: label('interruptUnavailable'),
    interrupting: label('interrupting'),
    jumpToLatest: label('jumpToLatest'),
    loadMoreConversations: label('loadMoreConversations'),
    renameConversation: label('renameConversation'),
    renameConversationConfirm: label('renameConversationConfirm'),
    newMessages: label('newMessages'),
    noConversations: label('noConversations'),
    openEventDetail: label('openEventDetail'),
    retry: label('retry'),
    runStatusLabel: label('runStatusLabel'),
    send: label('send'),
    sending: label('sending'),
    textSendingUnavailable: label('textSendingUnavailable'),
    timelineLabel: label('timelineLabel'),
    lifecycleStatus: {
      degraded: label('lifecycleDegraded'),
    },
  };
}
