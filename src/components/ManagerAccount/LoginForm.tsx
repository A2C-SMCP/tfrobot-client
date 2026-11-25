import { Form, Button, Card, Alert, Typography, Space, Spin } from 'antd';
import { useEffect } from 'react';
import { Input } from '@/components/common/Input';
import { LoginOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { managerCustomAddress, managerEnvironmentKind, useManagerStore, type ManagerError } from '@/stores/managerStore';

const { Title, Text } = Typography;

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
  const {
    login,
    identityLoading,
    identityError,
    clearError,
    loginEnvironment,
    loginEnvironmentLoadFailed,
    loginEnvironmentChanging,
    loadLoginEnvironment,
  } = useManagerStore();
  const customAddress = managerCustomAddress(loginEnvironment);
  const environmentKind = loginEnvironment && managerEnvironmentKind(loginEnvironment);

  useEffect(() => {
    void loadLoginEnvironment();
  }, [loadLoginEnvironment]);

  useEffect(() => {
    form.resetFields(['password']);
  }, [form, environmentKind, customAddress]);

  const handleFinish = async ({ identifier, password }: { identifier: string; password: string }) => {
    if (!loginEnvironment || loginEnvironmentChanging) return;
    clearError();
    try {
      await login(loginEnvironment, identifier, password);
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
          {loginEnvironmentLoadFailed && <Alert
            type="error"
            message={t('managerAccount.login.environmentLoadFailed')}
            action={<Button onClick={() => void loadLoginEnvironment()}>{t('common.retry')}</Button>}
          />}
          <Form.Item
            label={t('managerAccount.login.environment')}
            extra={customAddress?.startsWith('http://')
              ? t('managerAccount.login.insecureCustomAddress')
              : t('managerAccount.login.environmentHint')}
          >
            {environmentKind
              ? <Space direction="vertical" size={0}>
                <Text>{t(`managerAccount.login.environments.${environmentKind}`)}</Text>
                {customAddress && <Text style={{ overflowWrap: 'anywhere' }}>{customAddress}</Text>}
              </Space>
              : !loginEnvironmentLoadFailed && <Spin size="small" />}
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
              disabled={!loginEnvironment || loginEnvironmentChanging || loginEnvironmentLoadFailed}
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
