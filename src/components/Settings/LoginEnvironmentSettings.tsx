import { Alert, Button, Form, Select, Space } from 'antd';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/common/Input';
import { PageModal } from '@/components/Navigation/PageOverlays';
import {
  MANAGER_ENVIRONMENTS,
  managerCustomAddress,
  managerEnvironmentKind,
  useManagerStore,
  type ManagerEnvironment,
  type ManagerEnvironmentKind,
} from '@/stores/managerStore';

function parseCustomAddress(value: string): string | null {
  const input = value.trim();
  if (!/^https?:\/\//i.test(input) || /[?#]/.test(input)) return null;
  try {
    const url = new URL(input);
    if (!url.hostname || url.username || url.password) return null;
    return url.href.replace(/\/+$/, '');
  } catch {
    return null;
  }
}

export function LoginEnvironmentSettings() {
  const { t } = useTranslation();
  const {
    context,
    identityLoading,
    loginEnvironment,
    loginEnvironmentChanging,
    loginEnvironmentLoadFailed,
    loadLoginEnvironment,
    changeLoginEnvironment,
  } = useManagerStore();
  const [pending, setPending] = useState<{ environment: ManagerEnvironment; revision: number } | null>(null);
  const [saveError, setSaveError] = useState(false);
  const [customAddressDraft, setCustomAddressDraft] = useState<string | null>(null);
  const [addressError, setAddressError] = useState(false);
  const savedAddress = managerCustomAddress(loginEnvironment);
  const customAddress = customAddressDraft ?? savedAddress;
  const busy = identityLoading || loginEnvironmentChanging;

  useEffect(() => {
    void loadLoginEnvironment();
  }, [loadLoginEnvironment]);

  const environmentLabel = (kind: ManagerEnvironmentKind) => t(`managerAccount.login.environments.${kind}`);

  const save = async (environment: ManagerEnvironment, revision: number | null) => {
    setSaveError(false);
    try {
      await changeLoginEnvironment(environment, revision);
      setCustomAddressDraft(null);
    } catch {
      setSaveError(true);
    } finally {
      setPending(null);
    }
  };

  const requestSave = (environment: ManagerEnvironment) => {
    if (context.authState === 'signed_out') {
      void save(environment, null);
    } else {
      setPending({ environment, revision: context.revision });
    }
  };

  const selectEnvironment = (kind: ManagerEnvironmentKind) => {
    setAddressError(false);
    if (kind === 'custom') {
      setCustomAddressDraft(customAddress ?? '');
      return;
    }
    setCustomAddressDraft(null);
    if (kind !== loginEnvironment) requestSave(kind);
  };

  const saveCustomAddress = () => {
    if (customAddress === null || busy) return;
    const normalized = parseCustomAddress(customAddress);
    if (!normalized) {
      setAddressError(true);
      return;
    }
    if (normalized === savedAddress) {
      setCustomAddressDraft(null);
      return;
    }
    requestSave({ custom: normalized });
  };

  const pendingAddress = pending && managerCustomAddress(pending.environment);

  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      {loginEnvironmentLoadFailed && (
        <Alert
          type="error"
          message={t('managerAccount.login.environmentLoadFailed')}
          action={<Button onClick={() => void loadLoginEnvironment()}>{t('common.retry')}</Button>}
        />
      )}
      {saveError && <Alert type="error" message={t('settings.loginEnvironmentSaveFailed')} />}
      <Form layout="vertical" onFinish={saveCustomAddress}>
        <Form.Item label={t('settings.loginEnvironment')} htmlFor="manager-login-environment"
          extra={t('settings.loginEnvironmentHint')}>
          <Select
            id="manager-login-environment"
            value={customAddress !== null ? 'custom' : loginEnvironment ? managerEnvironmentKind(loginEnvironment) : undefined}
            disabled={busy || (!loginEnvironment && !loginEnvironmentLoadFailed)}
            loading={loginEnvironmentChanging || (!loginEnvironment && !loginEnvironmentLoadFailed)}
            options={MANAGER_ENVIRONMENTS.map((value) => ({ value, label: environmentLabel(value) }))}
            onChange={selectEnvironment}
          />
        </Form.Item>
        {customAddress !== null && (
          <>
            <Form.Item
              label={t('settings.managerCustomAddress')}
              htmlFor="manager-custom-address"
              validateStatus={addressError ? 'error' : undefined}
              help={addressError
                ? t('settings.managerCustomAddressInvalid')
                : customAddress.trim().toLowerCase().startsWith('http://')
                  ? t('managerAccount.login.insecureCustomAddress')
                  : undefined}
            >
              <Input
                id="manager-custom-address"
                value={customAddress}
                disabled={busy}
                placeholder="https://manager.example.com"
                onChange={(event) => {
                  setCustomAddressDraft(event.target.value);
                  setAddressError(false);
                }}
              />
            </Form.Item>
            <Button type="primary" htmlType="submit" disabled={busy || customAddress === savedAddress}>
              {t('common.save')}
            </Button>
          </>
        )}
      </Form>
      <PageModal
        open={pending !== null}
        title={t('settings.loginEnvironmentConfirmTitle')}
        okText={t('settings.loginEnvironmentConfirm')}
        cancelText={t('common.cancel')}
        confirmLoading={loginEnvironmentChanging}
        closable={!loginEnvironmentChanging}
        maskClosable={!loginEnvironmentChanging}
        keyboard={!loginEnvironmentChanging}
        cancelButtonProps={{ disabled: loginEnvironmentChanging }}
        onCancel={() => setPending(null)}
        onOk={() => { if (pending) void save(pending.environment, pending.revision); }}
      >
        {pending && t('settings.loginEnvironmentConfirmDescription', {
          environment: pendingAddress
            ? `${environmentLabel('custom')} (${pendingAddress})`
            : environmentLabel(managerEnvironmentKind(pending.environment)),
        })}
      </PageModal>
    </Space>
  );
}
