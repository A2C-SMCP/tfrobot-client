import { Alert, Button, Form, Select, Space } from 'antd';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageModal } from '@/components/Navigation/PageOverlays';
import { MANAGER_ENVIRONMENTS, useManagerStore, type ManagerEnvironment } from '@/stores/managerStore';

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

  useEffect(() => {
    void loadLoginEnvironment();
  }, [loadLoginEnvironment]);

  const environmentLabel = (environment: ManagerEnvironment) =>
    t(`managerAccount.login.environments.${environment}`);

  const save = async (environment: ManagerEnvironment, revision: number | null) => {
    setSaveError(false);
    try {
      await changeLoginEnvironment(environment, revision);
    } catch {
      setSaveError(true);
    } finally {
      setPending(null);
    }
  };

  const selectEnvironment = (environment: ManagerEnvironment) => {
    if (environment === loginEnvironment) return;
    if (context.authState === 'signed_out') {
      void save(environment, null);
    } else {
      setPending({ environment, revision: context.revision });
    }
  };

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
      <Form layout="vertical">
        <Form.Item label={t('settings.loginEnvironment')} htmlFor="manager-login-environment"
          extra={t('settings.loginEnvironmentHint')}>
          <Select
            id="manager-login-environment"
            value={loginEnvironment ?? undefined}
            disabled={!loginEnvironment || identityLoading || loginEnvironmentChanging}
            loading={loginEnvironmentChanging || (!loginEnvironment && !loginEnvironmentLoadFailed)}
            options={MANAGER_ENVIRONMENTS.map((value) => ({ value, label: environmentLabel(value) }))}
            onChange={selectEnvironment}
          />
        </Form.Item>
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
          environment: environmentLabel(pending.environment),
        })}
      </PageModal>
    </Space>
  );
}
