import { Form, Button, Card, Alert, Typography, Space, Select, Spin } from 'antd';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Input } from '@/components/common/Input';
import { LoginOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  useManagerStore,
  type ManagerEnvironment,
  type ManagerError,
  type ManagerEnvironmentPolicy,
} from '@/stores/managerStore';

const { Title, Text } = Typography;
const ENVIRONMENT_PREFERENCE = 'tfrobot-client.development.managerEnvironment';

function isEnvironmentPolicy(value: unknown): value is ManagerEnvironmentPolicy {
  if (!value || typeof value !== 'object') return false;
  const policy = value as Partial<ManagerEnvironmentPolicy>;
  const allowed = policy.allowedEnvironments;
  return Array.isArray(allowed) && allowed.length > 0
    && allowed.every((environment) => ['test', 'staging', 'prod'].includes(environment))
    && allowed.includes(policy.defaultEnvironment as ManagerEnvironment)
    && (policy.buildMode === 'development'
      || (policy.buildMode === 'production' && allowed.length === 1 && allowed[0] === 'prod'));
}

function preferredEnvironment(policy: ManagerEnvironmentPolicy, current: ManagerEnvironment | null) {
  if (current && policy.allowedEnvironments.includes(current)) return current;
  if (policy.buildMode === 'development') {
    try {
      const saved = localStorage.getItem(ENVIRONMENT_PREFERENCE) as ManagerEnvironment | null;
      if (saved && policy.allowedEnvironments.includes(saved)) return saved;
    } catch { /* Optional UI preference; authentication never depends on browser storage. */ }
  }
  return policy.defaultEnvironment;
}

function errorI18nKey(err: ManagerError): string {
  return `manager.errors.${err.kind}`;
}

interface LoginFormProps {
  onSubmitted?: () => void;
  embedded?: boolean;
}

export function LoginForm({ onSubmitted, embedded = false }: LoginFormProps) {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const { context, login, identityLoading, identityError, clearError } = useManagerStore();
  const [policy, setPolicy] = useState<ManagerEnvironmentPolicy | null>(null);
  const [policyFailed, setPolicyFailed] = useState(false);
  const [policyAttempt, setPolicyAttempt] = useState(0);
  useEffect(() => {
    let disposed = false;
    setPolicy(null);
    setPolicyFailed(false);
    void invoke<unknown>('get_manager_environment_policy').then((value) => {
      if (!isEnvironmentPolicy(value)) throw new Error('Invalid Manager environment policy');
      if (disposed) return;
      form.setFieldsValue({ environment: preferredEnvironment(value, context.environment) });
      setPolicy(value);
    }).catch(() => { if (!disposed) setPolicyFailed(true); });
    return () => { disposed = true; };
  }, [form, context.environment, policyAttempt]);

  const handleFinish = async (values: {
    environment: ManagerEnvironment;
    identifier: string;
    password: string;
  }) => {
    if (!policy || policyFailed || !policy.allowedEnvironments.includes(values.environment)) return;
    clearError();
    try {
      await login(values.environment, values.identifier, values.password);
      onSubmitted?.();
    } catch {
      /* error already stored */
    }
  };

  const content = (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>
            {t('managerAccount.login.title')}
          </Title>
          <Text type="secondary">{t('managerAccount.login.description')}</Text>
        </div>

        {identityError && (
          <Alert
            type="error"
            showIcon
            message={t(errorI18nKey(identityError))}
            description={
              identityError.kind === 'network_error'
                ? identityError.detail
                : identityError.kind === 'invalid_credentials'
                ? identityError.detail.message
                : identityError.kind === 'other'
                ? `HTTP ${identityError.detail.status}: ${identityError.detail.body}`
                : undefined
            }
            closable
            onClose={clearError}
          />
        )}

        <Form
          form={form}
          layout="vertical"
          onFinish={handleFinish}
        >
          {policyFailed && <Alert
            type="error"
            message={t('managerAccount.login.environmentPolicyFailed')}
            action={<Button onClick={() => setPolicyAttempt((attempt) => attempt + 1)}>{t('common.retry')}</Button>}
          />}
          {policy?.buildMode === 'production' && <Form.Item name="environment" hidden><Input /></Form.Item>}
          <Form.Item
            name={policy?.buildMode === 'development' ? 'environment' : undefined}
            label={t('managerAccount.login.environment')}
            extra={policy?.buildMode === 'production'
              ? t('managerAccount.login.productionOnly')
              : t('managerAccount.login.environmentHint')}
            rules={policy?.buildMode === 'development' ? [{ required: true, message: t('managerAccount.login.environmentRequired') }] : undefined}
          >
            {policy?.buildMode === 'development' ? <Select
              disabled={identityLoading}
              onChange={(value: ManagerEnvironment) => {
                try { localStorage.setItem(ENVIRONMENT_PREFERENCE, value); }
                catch { /* The selection still applies when optional storage is unavailable. */ }
              }}
              options={policy.allowedEnvironments.map((value) => ({ value, label: t(`managerAccount.login.environments.${value}`) }))}
            /> : policy ? <Text>{t('managerAccount.login.environments.prod')}</Text> : <Spin size="small" />}
          </Form.Item>
          <Form.Item
            name="identifier"
            label={t('managerAccount.login.identifier')}
            rules={[{ required: true, message: t('managerAccount.login.identifierRequired') }]}
          >
            <Input autoComplete="username" />
          </Form.Item>
          <Form.Item
            name="password"
            label={t('managerAccount.login.password')}
            rules={[{ required: true, message: t('managerAccount.login.passwordRequired') }]}
          >
            <Input.Password autoComplete="current-password" />
          </Form.Item>
          <Form.Item>
            <Button
              type="primary"
              htmlType="submit"
              icon={<LoginOutlined />}
              loading={identityLoading}
              disabled={!policy || policyFailed}
              block
            >
              {t('managerAccount.login.submit')}
            </Button>
          </Form.Item>
        </Form>
    </Space>
  );
  return embedded ? content : <Card>{content}</Card>;
}
