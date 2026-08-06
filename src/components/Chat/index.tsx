import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { LoginOutlined, MessageOutlined, ReloadOutlined } from '@ant-design/icons';
import {
  Alert,
  Button,
  Card,
  Empty,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
} from 'antd';
import {
  ChatWorkspace,
  OwnedChatProvider,
  type ChatError,
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
import { chatRobotDisabledReason } from './availability';
import styles from './Chat.module.css';

const { Text, Title } = Typography;
const EMPTY_EMPLOYEES: DigitalEmployeeBrief[] = [];

type Translator = (key: string, options?: Record<string, unknown>) => string;

function managerErrorText(t: Translator, error: ManagerError | null): string | null {
  return error === null ? null : t(`manager.errors.${error.kind}`);
}

interface ActiveChatProps {
  descriptor: ChatSessionDescriptor;
  creator: { uid: string; name: string };
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

  return (
    <OwnedChatProvider
      factory={factory}
      fallback={<div className={styles.centered}><Spin /></div>}
      onDisposeError={() => warn('chat: failed to dispose ChatClient cleanly')}
    >
      <ChatWorkspace
        allowCreate
        getDeadlineAt={getChatDeadlineAt}
        labels={labels}
        pageSize={50}
        sidebarTitle={t('chat.conversations')}
        sidebarWidth={220}
      />
    </OwnedChatProvider>
  );
}

interface ChatSessionHostProps {
  creator: { uid: string; name: string };
  employeeId: number;
}

function ChatSessionHost({ creator, employeeId }: ChatSessionHostProps) {
  const { t } = useTranslation();
  const [descriptor, setDescriptor] = useState<ChatSessionDescriptor | null>(null);
  const [error, setError] = useState<ManagerError | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    let leaseId: string | null = null;
    setDescriptor(null);
    setError(null);
    void invoke<ChatSessionDescriptor>('chat_open_session', { employeeId })
      .then((opened) => {
        leaseId = opened.leaseId;
        if (!active) {
          return invoke('chat_close_session', { leaseId: opened.leaseId });
        }
        setDescriptor(opened);
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
  }, [attempt, employeeId]);

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
  const { t } = useTranslation();
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
  const [selectedEmployeeId, setSelectedEmployeeId] = useState<number | null>(null);
  const creator = useMemo(() => context.account === null ? null : ({
    uid: context.account.id,
    name: context.account.nickname || context.account.name,
  }), [context.account]);

  useEffect(() => {
    setSelectedEmployeeId(null);
    if (scope) {
      void fetchEmployeesIfStale().catch(() => undefined);
    }
  }, [fetchEmployeesIfStale, scope]);

  useEffect(() => {
    if (
      selectedEmployeeId !== null
      && !employees.some((employee) => employee.id === selectedEmployeeId
        && chatRobotDisabledReason(employee) === null)
    ) {
      setSelectedEmployeeId(null);
    }
  }, [employees, selectedEmployeeId]);

  if (context.authState !== 'authenticated' || creator === null) {
    return (
      <div className={styles.centered}>
        <Empty
          image={<LoginOutlined style={{ fontSize: 48 }} />}
          description={t('chat.signInRequired')}
        />
      </div>
    );
  }

  const resourceError = resource?.error ?? identityError;
  const options = employees.map((employee) => {
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
    <div className={styles.page}>
      <div className={styles.pageHeader}>
        <div>
          <Title level={3} style={{ marginBottom: 4 }}>
            <MessageOutlined /> {t('chat.title')}
          </Title>
          <Text type="secondary">
            {t('chat.context', { organization: context.organization?.name ?? '' })}
          </Text>
        </div>
        <Space className={styles.selector}>
          <Select<number>
            aria-label={t('chat.selectRobot')}
            placeholder={t('chat.selectRobot')}
            value={selectedEmployeeId ?? undefined}
            options={options}
            loading={resource?.loading === true}
            onChange={setSelectedEmployeeId}
            style={{ flex: 1, minWidth: 0 }}
          />
          <Button
            aria-label={t('common.refresh')}
            icon={<ReloadOutlined />}
            loading={resource?.loading === true}
            onClick={() => { void fetchEmployees().catch(() => undefined); }}
          />
        </Space>
      </div>

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
          {selectedEmployeeId === null ? (
            <div className={styles.centered}>
              <Empty description={t('chat.chooseRobot')} />
            </div>
          ) : (
            <ChatSessionHost
              key={`${scope}:${selectedEmployeeId}`}
              creator={creator}
              employeeId={selectedEmployeeId}
            />
          )}
        </div>
      </Card>
    </div>
  );
}
