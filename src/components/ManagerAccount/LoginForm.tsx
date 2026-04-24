import { useEffect } from 'react';
import { Form, Input, Button, Card, Alert, Typography, Space } from 'antd';
import { LoginOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useManagerStore, type ManagerError } from '@/stores/managerStore';

const { Title, Text } = Typography;

function errorI18nKey(err: ManagerError): string {
  return `manager.errors.${err.kind}`;
}

interface LoginFormProps {
  onSubmitted?: () => void;
}

export function LoginForm({ onSubmitted }: LoginFormProps) {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const { baseUrl, login, loading, error, clearError, setBaseUrl } = useManagerStore();

  useEffect(() => {
    form.setFieldValue('baseUrl', baseUrl);
  }, [baseUrl, form]);

  const handleFinish = async (values: { baseUrl?: string; phone: string; password: string }) => {
    clearError();
    try {
      if (values.baseUrl) setBaseUrl(values.baseUrl);
      await login(values.phone, values.password, values.baseUrl);
      onSubmitted?.();
    } catch {
      /* error already stored */
    }
  };

  return (
    <Card>
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>
            {t('managerAccount.login.title')}
          </Title>
          <Text type="secondary">{t('managerAccount.login.description')}</Text>
        </div>

        {error && (
          <Alert
            type="error"
            showIcon
            message={t(errorI18nKey(error))}
            description={
              error.kind === 'network_error'
                ? error.detail
                : error.kind === 'other'
                ? `HTTP ${error.detail.status}: ${error.detail.body}`
                : undefined
            }
            closable
            onClose={clearError}
          />
        )}

        <Form form={form} layout="vertical" onFinish={handleFinish}>
          <Form.Item
            name="baseUrl"
            label={t('managerAccount.login.baseUrl')}
            extra={t('managerAccount.login.baseUrlHint')}
          >
            <Input placeholder="https://manager.example.com" />
          </Form.Item>
          <Form.Item
            name="phone"
            label={t('managerAccount.login.phone')}
            rules={[{ required: true, message: t('managerAccount.login.phoneRequired') }]}
          >
            <Input autoComplete="username" inputMode="tel" />
          </Form.Item>
          <Form.Item
            name="password"
            label={t('managerAccount.login.password')}
            rules={[{ required: true, message: t('managerAccount.login.passwordRequired') }]}
          >
            <Input.Password autoComplete="current-password" />
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit" icon={<LoginOutlined />} loading={loading} block>
              {t('managerAccount.login.submit')}
            </Button>
          </Form.Item>
        </Form>
      </Space>
    </Card>
  );
}
