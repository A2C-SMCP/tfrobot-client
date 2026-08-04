import {
  Alert,
  Avatar,
  Button,
  Descriptions,
  Divider,
  List,
  Popover,
  Space,
  Spin,
  Tag,
  Typography,
} from 'antd';
import {
  CheckOutlined,
  DownOutlined,
  LoginOutlined,
  LogoutOutlined,
  SwapOutlined,
  UserOutlined,
} from '@ant-design/icons';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  managerContextScope,
  useManagerStore,
  type ManagerError,
} from '@/stores/managerStore';
import { AccountSelection } from './AccountSelection';
import { LoginForm } from './LoginForm';
import styles from './GlobalManagerAccount.module.css';

const { Text } = Typography;

function managerErrorKey(error: ManagerError): string {
  return `manager.errors.${error.kind}`;
}

export function GlobalManagerAccount() {
  const { t } = useTranslation();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const {
    context,
    pendingAccountSelection,
    identityLoading,
    identityError,
    availableAccounts,
    accountsLoading,
    fetchAccounts,
    switchAccount,
    logout,
    clearError,
  } = useManagerStore();
  const authenticated = context.authState === 'authenticated'
    && context.account !== null
    && context.organization !== null;
  const authenticatedScope = managerContextScope(context);

  useEffect(() => {
    if (!open || authenticatedScope === null) return;
    void fetchAccounts().catch(() => {
      /* the authoritative store exposes the error in this panel */
    });
  }, [authenticatedScope, fetchAccounts, open]);

  useEffect(() => {
    if (!open) return undefined;
    const frame = window.requestAnimationFrame(() => {
      panelRef.current
        ?.querySelector<HTMLElement>('input, button:not([disabled]), [tabindex="0"]')
        ?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [open, context.authState]);

  const updateOpen = (next: boolean) => {
    setOpen(next);
    if (!next) window.requestAnimationFrame(() => triggerRef.current?.focus());
  };

  const handleSwitch = async (accountId: string) => {
    clearError();
    try {
      await switchAccount(accountId);
    } catch {
      /* the authoritative store exposes the error in this panel */
    }
  };

  const handleLogout = async () => {
    clearError();
    try {
      await logout();
    } catch {
      // Logout can finish locally while reporting teardown diagnostics. Context refresh in the
      // store still moves this panel to signed-out and retains the diagnostic for the user.
    }
  };

  const authenticatedPanel = authenticated && context.account && context.organization ? (
    <>
      <Space align="center" className={styles.identityHeader}>
        <Avatar src={context.account.avatar || undefined} icon={<UserOutlined />} />
        <div className={styles.identityText} aria-live="polite">
          <Text strong ellipsis>{context.organization.name}</Text>
          <Text type="secondary" ellipsis>{context.account.nickname || context.account.name}</Text>
        </div>
      </Space>
      <Descriptions
        size="small"
        column={1}
        className={styles.details}
        items={[
          {
            key: 'environment',
            label: t('managerAccount.global.environment'),
            children: context.environment
              ? t(`managerAccount.login.environments.${context.environment}`)
              : '—',
          },
          {
            key: 'organization',
            label: t('managerAccount.global.organization'),
            children: context.organization.name,
          },
          {
            key: 'account',
            label: t('managerAccount.global.account'),
            children: context.account.name,
          },
        ]}
      />
      <Divider className={styles.divider} />
      <Space direction="vertical" size={8} className={styles.section}>
        <Text strong>{t('managerAccount.global.switchAccount')}</Text>
        {accountsLoading && availableAccounts === null ? (
          <div className={styles.loading} role="status">
            <Spin size="small" />
            <Text type="secondary">{t('managerAccount.global.loadingAccounts')}</Text>
          </div>
        ) : (
          <List
            size="small"
            className={styles.accountList}
            locale={{ emptyText: t('managerAccount.global.noAccounts') }}
            dataSource={availableAccounts ?? []}
            rowKey={(account) => `${account.organizationId}:${account.accountId}`}
            renderItem={(account) => {
              const current = account.accountId === context.account?.id
                && account.organizationId === context.organization?.id;
              return (
                <List.Item className={styles.accountItem}>
                  <Button
                    type="text"
                    className={styles.accountAction}
                    disabled={identityLoading || current}
                    aria-current={current ? 'true' : undefined}
                    onClick={() => void handleSwitch(account.accountId)}
                  >
                    <span className={styles.accountActionText}>
                      <span className={styles.accountName}>
                        {account.nickname || account.accountName}
                      </span>
                      <span className={styles.organizationName}>{account.organizationName}</span>
                    </span>
                    {current ? (
                      <Tag icon={<CheckOutlined />} color="success">
                        {t('managerAccount.global.current')}
                      </Tag>
                    ) : <SwapOutlined />}
                  </Button>
                </List.Item>
              );
            }}
          />
        )}
      </Space>
      <Divider className={styles.divider} />
      <div className={styles.logoutArea}>
        <Button
          color="danger"
          variant="filled"
          block
          className={styles.logoutButton}
          icon={<LogoutOutlined />}
          loading={identityLoading}
          onClick={() => void handleLogout()}
        >
          {t('managerAccount.login.logout')}
        </Button>
      </div>
    </>
  ) : null;

  let panelContent;
  if (context.authState === 'account_selection_required' && pendingAccountSelection) {
    panelContent = <AccountSelection embedded />;
  } else if (context.authState === 'onboarding_required') {
    panelContent = (
      <Space direction="vertical" size="middle" className={styles.section}>
        <Alert
          type="info"
          showIcon
          message={t('managerAccount.onboarding.title')}
          description={t('managerAccount.onboarding.description')}
        />
        <Button loading={identityLoading} onClick={() => void handleLogout()} block>
          {t('managerAccount.onboarding.back')}
        </Button>
      </Space>
    );
  } else if (authenticatedPanel) {
    panelContent = authenticatedPanel;
  } else {
    panelContent = <LoginForm embedded />;
  }

  const triggerLabel = authenticated && context.account && context.organization
    ? `${context.organization.name}, ${context.account.nickname || context.account.name}`
    : context.authState === 'account_selection_required'
    ? t('managerAccount.global.completeSignIn')
    : context.authState === 'onboarding_required'
    ? t('managerAccount.global.setupRequired')
    : identityError?.kind === 'unauthorized'
    ? t('managerAccount.global.sessionExpired')
    : t('managerAccount.login.submit');

  return (
    <Popover
      open={open}
      onOpenChange={updateOpen}
      trigger="click"
      placement="bottomRight"
      destroyTooltipOnHide
      content={(
        <div
          id="global-manager-account-panel"
          ref={panelRef}
          className={styles.panel}
          role="dialog"
          aria-label={t('managerAccount.navLabel')}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              event.stopPropagation();
              updateOpen(false);
            }
          }}
        >
          {identityError && authenticated && (
            <Alert
              className={styles.error}
              type="error"
              showIcon
              closable
              message={t(managerErrorKey(identityError))}
              onClose={clearError}
            />
          )}
          {panelContent}
        </div>
      )}
    >
      <Button
        ref={triggerRef}
        type="text"
        className={styles.trigger}
        icon={authenticated ? (
          <Avatar
            size="small"
            src={context.account?.avatar || undefined}
            icon={<UserOutlined />}
          />
        ) : <LoginOutlined />}
        loading={identityLoading}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls="global-manager-account-panel"
        title={triggerLabel}
      >
        <span className={styles.triggerText}>{triggerLabel}</span>
        <DownOutlined className={styles.chevron} />
      </Button>
    </Popover>
  );
}
