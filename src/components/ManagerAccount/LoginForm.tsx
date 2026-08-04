import { Form, Button, Card, Alert, Typography, Space, Select } from 'antd';
import { Input } from '@/components/common/Input';
import { LoginOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  useManagerStore,
  type ManagerEnvironment,
  type ManagerError,
} from '@/stores/managerStore';

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
  const { context, login, identityLoading, identityError, clearError } = useManagerStore();

  const handleFinish = async (values: {
    environment: ManagerEnvironment;
    identifier: string;
    password: string;
  }) => {
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
          initialValues={{ environment: context.environment ?? 'staging' }}
          onFinish={handleFinish}
        >
          <Form.Item
            name="environment"
            label={t('managerAccount.login.environment')}
            extra={t('managerAccount.login.environmentHint')}
            rules={[{ required: true, message: t('managerAccount.login.environmentRequired') }]}
          >
            <Select
              options={(['staging', 'beta', 'prod'] as ManagerEnvironment[]).map((value) => ({
                value,
                label: t(`managerAccount.login.environments.${value}`),
              }))}
            />
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
