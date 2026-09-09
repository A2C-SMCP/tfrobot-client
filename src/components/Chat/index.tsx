import { useCallback, useEffect, useMemo, useState, type CSSProperties } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  ApartmentOutlined,
  LoginOutlined,
  MessageOutlined,
  ReloadOutlined,
  RobotOutlined,
} from '@ant-design/icons';
import {
  Alert,
  Button,
  Card,
  Empty,
  Input,
  Modal,
  Select,
  Space,
  Spin,
  Tag,
  Tooltip,
  Typography,
  theme,
} from 'antd';
import {
  ChatConversationView,
  ChatResourceProvider,
  ChatUiShell,
  OwnedChatProvider,
  useConversationWorkspace,
  type ChatContentState,
  type ChatError,
  type ChatUiLabelOverrides,
  type ConversationWorkspaceBinding,
} from '@turingfocus/chat-kit';
import { useTranslation } from 'react-i18next';

import {
  currentEmployeeResource,
  managerContextScope,
  useManagerStore,
  type DigitalEmployeeBrief,
  type ManagerError,
} from '@/stores/managerStore';
import { warn } from '@/utils/logger';
import {
  chatUiLabels,
  createClientChatFactory,
  getChatDeadlineAt,
  type ChatSessionDescriptor,
} from './chatBridge';
import {
  chatRobotDisabledReason,
  orderChatRobots,
  preferredChatRobotId,
} from './availability';
import styles from './Chat.module.css';
import { createChatResourcePort } from './chatResources';

const { Text, Title } = Typography;
const EMPTY_EMPLOYEES: DigitalEmployeeBrief[] = [];
let lastSelectionRevision = 0;

type Translator = (key: string, options?: Record<string, unknown>) => string;
type ChatThemeStyle = CSSProperties & Record<`--chat-${string}`, string>;
type ChatSelection = { scope: string; employeeId: number };

function nextSelectionRevision(): number {
  lastSelectionRevision = Math.max(lastSelectionRevision + 1, Date.now() * 1_000);
  return lastSelectionRevision;
}

function managerErrorText(t: Translator, error: ManagerError | null): string | null {
  return error === null ? null : t(`manager.errors.${error.kind}`);
}

interface ActiveChatProps {
  descriptor: ChatSessionDescriptor;
  creator: { uid: string; name: string };
}

type WorkspaceSnapshot = ConversationWorkspaceBinding['snapshot'];

function contentStateFor(
  ready: boolean,
  snapshot: WorkspaceSnapshot,
  refresh: () => void,
  retrySelection: (() => void) | undefined,
): ChatContentState {
  if (!ready) return { kind: 'loading' };
  if (snapshot.selectionStatus === 'ready') return { kind: 'ready' };
  if (snapshot.selectionStatus === 'loading') return { kind: 'loading' };
  if (snapshot.selectionStatus === 'error') {
    const error = snapshot.selectionError;
    return {
      kind: error?.code === 'network' ? 'disconnected' : 'error',
      ...(error === undefined ? {} : { description: error.message }),
      ...(retrySelection === undefined ? {} : { onRetry: retrySelection }),
    };
  }
  if (snapshot.listStatus === 'error' && snapshot.selectedConversationId === undefined) {
    return {
      kind: snapshot.listError?.code === 'network' ? 'disconnected' : 'error',
      ...(snapshot.listError === undefined ? {} : { description: snapshot.listError.message }),
      onRetry: refresh,
    };
  }
  if (snapshot.listStatus === 'loading' || snapshot.listStatus === 'idle') {
    return { kind: 'loading' };
  }
  return { kind: 'empty' };
}

function ActiveChat({ descriptor, creator }: ActiveChatProps) {
  const { t } = useTranslation();
  const labels = useMemo(() => chatUiLabels((key) => t(key)), [t]);
  const factory = useMemo(() => createClientChatFactory({
    descriptor,
    messageCreator: creator,
    onDiagnostic: (error: ChatError) => {
      warn(`chat: diagnostic code=${error.code}`);
    },
    onUnhandledError: () => {
      warn('chat: unhandled client callback error');
    },
  }), [creator, descriptor]);

  const [resourceError, setResourceError] = useState<string>();
  const resources = useMemo(() => createChatResourcePort(descriptor.leaseId, setResourceError), [descriptor.leaseId]);
  useEffect(() => {
    let disposed = false;
    const listener = listen<{ leaseId: string; error: { code: string } }>('chat-resource-error', ({ payload }) => {
      if (!disposed && payload.leaseId === descriptor.leaseId) setResourceError(payload.error.code);
    });
    return () => { disposed = true; void listener.then((unlisten) => unlisten()).catch(() => undefined); };
  }, [descriptor.leaseId]);

  return (
    <OwnedChatProvider
      factory={factory}
      fallback={<div className={styles.centered}><Spin /></div>}
      onDisposeError={() => warn('chat: failed to dispose ChatClient cleanly')}
    >
      {resourceError && <Alert type="error" showIcon closable
        message={t(`chat.resourceErrors.${resourceError}`, { defaultValue: t('chat.resourceErrors.network') })}
        onClose={() => setResourceError(undefined)} />}
      <ChatResourceProvider port={resources} scope={descriptor.leaseId}>
        <CompactChatWorkspace labels={labels} />
      </ChatResourceProvider>
    </OwnedChatProvider>
  );
}

interface CompactChatWorkspaceProps {
  labels: ChatUiLabelOverrides;
}

/**
 * Renders the chat workspace with chat-kit's compact navigation: an inline
 * conversation title plus new/history entries in the header. Conversation
 * listing, creation, paging, selection and async races stay owned by the
 * workspace controller — only the navigation chrome is host-rendered.
 */
function CompactChatWorkspace({ labels }: CompactChatWorkspaceProps) {
  const workspace = useConversationWorkspace({
    getDeadlineAt: getChatDeadlineAt,
    initialSelection: 'first',
    onUnhandledError: () => warn('chat: unhandled workspace error'),
    pageSize: 50,
  });
  const [createOpen, setCreateOpen] = useState(false);
  const [title, setTitle] = useState('');
  const [historyOpen, setHistoryOpen] = useState(false);

  useEffect(() => {
    setCreateOpen(false);
    setTitle('');
  }, [workspace.controller]);

  const snapshot = workspace.snapshot;
  const refresh = useCallback(() => {
    void workspace.refresh();
  }, [workspace]);
  const retryConversationId = snapshot.selectionError?.conversationId ?? snapshot.pendingConversationId;
  const retrySelection = useMemo(
    () => (retryConversationId === undefined
      ? undefined
      : () => {
        void workspace.selectConversation(retryConversationId);
      }),
    [retryConversationId, workspace],
  );
  const contentState = contentStateFor(workspace.ready, snapshot, refresh, retrySelection);

  const create = useCallback(async () => {
    const normalizedTitle = title.trim();
    if (normalizedTitle.length === 0 || snapshot.creating) return;
    const result = await workspace.createConversation({ title: normalizedTitle });
    if (result?.ok === true) {
      setCreateOpen(false);
      setTitle('');
    }
  }, [title, workspace, snapshot.creating]);

  const selectedConversation = snapshot.selectedConversationId === undefined
    ? undefined
    : snapshot.conversations.find((item) => item.id === snapshot.selectedConversationId);
  const compactNavigation = {
    conversationTitle: selectedConversation?.title ?? labels.conversationListLabel ?? 'Conversations',
    conversationHistoryOpen: historyOpen,
    conversationHistoryItems: snapshot.conversations,
    conversationHistoryLoading: snapshot.listStatus === 'loading',
    conversationHistoryError: snapshot.listError?.message,
    onConversationHistoryOpenChange: setHistoryOpen,
    onNewConversation: () => setCreateOpen(true),
  };

  return (
    <>
      <ChatUiShell
        compactNavigation={compactNavigation}
        contentState={contentState}
        conversationListError={
          snapshot.listStatus !== 'error' || snapshot.listError === undefined
            ? undefined
            : { message: snapshot.listError.message, onRetry: refresh }
        }
        conversationListLoading={!workspace.ready || snapshot.listStatus === 'loading'}
        conversations={snapshot.conversations}
        labels={labels}
        navigationMode="compact"
        onConversationSelect={(conversationId) => {
          void workspace.selectConversation(conversationId);
        }}
        pendingConversationId={snapshot.pendingConversationId}
        selectedConversationId={snapshot.selectedConversationId}
      >
        <ChatConversationView getDeadlineAt={getChatDeadlineAt} labels={labels} />
      </ChatUiShell>
      <Modal
        cancelButtonProps={{ disabled: snapshot.creating }}
        closable={!snapshot.creating}
        confirmLoading={snapshot.creating}
        keyboard={!snapshot.creating}
        maskClosable={!snapshot.creating}
        okButtonProps={{ disabled: title.trim().length === 0 }}
        okText={labels.createConversationConfirm ?? 'Create'}
        onCancel={() => setCreateOpen(false)}
        onOk={() => { void create(); }}
        open={createOpen}
        title={labels.createConversation ?? 'New conversation'}
      >
        <Space direction="vertical" style={{ width: '100%' }}>
          {snapshot.creationError === undefined ? null : (
            <Alert message={snapshot.creationError.message} role="alert" showIcon type="error" />
          )}
          <label>
            <Typography.Text>
              {labels.createConversationTitleLabel ?? 'Conversation title'}
            </Typography.Text>
            <Input
              aria-label={labels.createConversationTitleLabel ?? 'Conversation title'}
              autoFocus
              disabled={snapshot.creating}
              onChange={(event) => setTitle(event.target.value)}
              onPressEnter={() => { void create(); }}
              placeholder={labels.createConversationTitlePlaceholder ?? 'Enter a conversation title'}
              value={title}
            />
          </label>
        </Space>
      </Modal>
    </>
  );
}

interface ChatSessionHostProps {
  creator: { uid: string; name: string };
  employeeId: number;
  onOpened: (employeeId: number) => void;
}

function ChatSessionHost({ creator, employeeId, onOpened }: ChatSessionHostProps) {
  const { t } = useTranslation();
  const [descriptor, setDescriptor] = useState<ChatSessionDescriptor | null>(null);
  const [error, setError] = useState<ManagerError | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    let leaseId: string | null = null;
    const selectionRevision = nextSelectionRevision();
    setDescriptor(null);
    setError(null);
    void invoke<ChatSessionDescriptor>('chat_open_session', { employeeId, selectionRevision })
      .then((opened) => {
        leaseId = opened.leaseId;
        if (!active) {
          return invoke('chat_close_session', { leaseId: opened.leaseId });
        }
        setDescriptor(opened);
        onOpened(employeeId);
        void invoke('chat_remember_robot', { leaseId: opened.leaseId }).catch(() => {
          warn('chat: failed to save recent Robot preference');
        });
        return undefined;
      })
      .catch((reason: ManagerError) => {
        if (active) setError(reason);
      });
    return () => {
      active = false;
      if (leaseId) {
        void invoke('chat_close_session', { leaseId }).catch(() => undefined);
      }
    };
  }, [attempt, employeeId, onOpened]);

  if (error) {
    return (
      <Alert
        type="error"
        showIcon
        message={t('chat.sessionFailed')}
        description={managerErrorText(t, error)}
        action={<Button onClick={() => setAttempt((value) => value + 1)}>{t('common.retry')}</Button>}
      />
    );
  }
  if (!descriptor) return <div className={styles.centered}><Spin /></div>;
  return <ActiveChat descriptor={descriptor} creator={creator} />;
}

export function Chat() {
  const { t, i18n } = useTranslation();
  const { token } = theme.useToken();
  const {
    context,
    employeeResources,
    identityError,
    fetchEmployees,
    fetchEmployeesIfStale,
  } = useManagerStore();
  const scope = managerContextScope(context);
  const resource = currentEmployeeResource({ context, employeeResources });
  const employees = resource?.employees ?? EMPTY_EMPLOYEES;
  const availableEmployeeCount = employees.filter(
    (employee) => chatRobotDisabledReason(employee) === null,
  ).length;
  const hasEmployeeSnapshot = resource?.lastFetchAt != null;
  const resourceError = resource?.error ?? identityError;
  const [selection, setSelection] = useState<ChatSelection | null>(null);
  const [recentEmployeeId, setRecentEmployeeId] = useState<number | null>(null);
  const [preferenceScope, setPreferenceScope] = useState<string | null>(null);
  const selectedEmployeeId = selection?.scope === scope ? selection.employeeId : null;
  const handleSessionOpened = useCallback((employeeId: number) => {
    setRecentEmployeeId(employeeId);
  }, []);
  const creator = useMemo(() => context.account === null ? null : ({
    uid: context.account.id,
    name: context.account.nickname || context.account.name,
  }), [context.account]);
  const chatThemeStyle: ChatThemeStyle = {
    '--chat-primary': token.colorPrimary,
    '--chat-primary-bg': token.colorPrimaryBg,
    '--chat-surface': token.colorBgContainer,
    '--chat-border': token.colorBorderSecondary,
    '--chat-text': token.colorText,
    '--chat-text-secondary': token.colorTextSecondary,
    '--chat-shadow': token.boxShadowTertiary,
  };

  useEffect(() => {
    setSelection(null);
    setRecentEmployeeId(null);
    setPreferenceScope(null);
    if (scope) {
      let active = true;
      void invoke<number | null>('chat_get_recent_robot')
        .then((employeeId) => {
          if (active) setRecentEmployeeId(employeeId ?? null);
        })
        .catch(() => {
          warn('chat: failed to load recent Robot preference');
        })
        .finally(() => {
          if (active) setPreferenceScope(scope);
        });
      void fetchEmployeesIfStale().catch(() => undefined);
      return () => {
        active = false;
      };
    }
    return undefined;
  }, [fetchEmployeesIfStale, scope]);

  const orderedEmployees = useMemo(() => orderChatRobots(
    employees,
    i18n.resolvedLanguage ?? i18n.language,
  ), [employees, i18n.language, i18n.resolvedLanguage]);
  const employeeListResolved = resource?.loading !== true
    && (hasEmployeeSnapshot || resourceError !== null);
  const selectionDecisionReady = scope !== null
    && preferenceScope === scope
    && employeeListResolved;

  useEffect(() => {
    if (!selectionDecisionReady) return;
    const preferred = preferredChatRobotId(
      orderedEmployees,
      selectedEmployeeId,
      recentEmployeeId,
    );
    if (preferred !== selectedEmployeeId) {
      setSelection(preferred === null || scope === null ? null : {
        scope,
        employeeId: preferred,
      });
    }
  }, [orderedEmployees, recentEmployeeId, scope, selectedEmployeeId, selectionDecisionReady]);

  if (context.authState !== 'authenticated' || creator === null) {
    return (
      <div className={styles.centered} data-testid="chat">
        <Empty
          image={<LoginOutlined style={{ fontSize: 48 }} />}
          description={t('chat.signInRequired')}
        />
      </div>
    );
  }

  const options = orderedEmployees.map((employee) => {
    const reason = chatRobotDisabledReason(employee);
    return {
      value: employee.id,
      disabled: reason !== null,
      label: (
        <Space size="small">
          <span>{employee.name}</span>
          {reason && <Tag>{t(`chat.robotDisabled.${reason}`)}</Tag>}
        </Space>
      ),
    };
  });

  return (
    <div className={styles.page} data-testid="chat" style={chatThemeStyle}>
      <section className={styles.hero}>
        <div className={styles.heroIdentity}>
          <div className={styles.heroIcon} aria-hidden="true">
            <MessageOutlined />
          </div>
          <div className={styles.heroCopy}>
            <Title level={3} className={styles.heroTitle}>{t('chat.title')}</Title>
            <div className={styles.contextTags}>
              <Tag bordered={false} icon={<ApartmentOutlined />}>
                {t('chat.context', { organization: context.organization?.name ?? '' })}
              </Tag>
              {hasEmployeeSnapshot && (
                <Tag bordered={false} color={availableEmployeeCount > 0 ? 'success' : 'default'}>
                  {t('chat.availableRobots', { count: availableEmployeeCount })}
                </Tag>
              )}
            </div>
          </div>
        </div>

        <div className={styles.selectorPanel}>
          <div className={styles.selectorControls}>
            <Select<number>
              aria-label={t('chat.selectRobot')}
              className={styles.robotSelect}
              size="large"
              placeholder={t('chat.selectRobot')}
              value={selectedEmployeeId ?? undefined}
              options={options}
              loading={resource?.loading === true}
              notFoundContent={t('chat.noAvailableRobots')}
              onChange={(employeeId) => {
                if (scope !== null) {
                  setSelection({ scope, employeeId });
                }
              }}
            />
            <Tooltip title={t('common.refresh')}>
              <Button
                aria-label={t('common.refresh')}
                className={styles.refreshButton}
                icon={<ReloadOutlined />}
                loading={resource?.loading === true}
                size="large"
                onClick={() => { void fetchEmployees().catch(() => undefined); }}
              />
            </Tooltip>
          </div>
        </div>
      </section>

      {resourceError && (
        <Alert
          type="error"
          showIcon
          message={t('chat.robotListFailed')}
          description={managerErrorText(t, resourceError)}
        />
      )}

      <Card className={styles.workspaceCard}>
        <div className={styles.workspace}>
          {selectedEmployeeId === null && !selectionDecisionReady ? (
            <div className={styles.centered}><Spin /></div>
          ) : selectedEmployeeId === null ? (
            <div className={styles.emptyState}>
              <div className={styles.emptyVisual} aria-hidden="true">
                <RobotOutlined />
              </div>
              <Title level={4} className={styles.emptyTitle}>{t('chat.emptyTitle')}</Title>
              <Text className={styles.emptyDescription}>{t('chat.chooseRobot')}</Text>
            </div>
          ) : (
            <ChatSessionHost
              key={`${scope}:${selectedEmployeeId}`}
              creator={creator}
              employeeId={selectedEmployeeId}
              onOpened={handleSessionOpened}
            />
          )}
        </div>
      </Card>
    </div>
  );
}
