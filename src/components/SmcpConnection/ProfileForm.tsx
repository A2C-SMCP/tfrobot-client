import { Form, Button, Space, Switch, Card } from 'antd';
import { Input } from '@/components/common/Input';
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { ConnectionProfile } from '@/stores/connectionStore';

interface ProfileFormProps {
  initialValues?: ConnectionProfile;
  onSubmit: (profile: ConnectionProfile, apiKey?: string) => Promise<void>;
  onCancel: () => void;
  loading?: boolean;
}

export function ProfileForm({ initialValues, onSubmit, onCancel, loading }: ProfileFormProps) {
  const { t } = useTranslation();
  const [form] = Form.useForm();

  const getInitialValues = () => {
    if (!initialValues) {
      return {
        namespace: '/smcp',
        auto_connect: true,
        auto_reconnect: true,
        headers: [],
      };
    }
    return {
      ...initialValues,
      headers: Object.entries(initialValues.headers || {}).map(([key, value]) => ({ key, value })),
    };
  };

  const handleFinish = async (values: Record<string, unknown>) => {
    const headersArr = (values.headers as { key: string; value: string }[]) || [];
    const headers: Record<string, string> = {};
    headersArr.forEach(({ key, value }) => {
      if (key) headers[key] = value;
    });

    const profile: ConnectionProfile = {
      name: values.name as string,
      url: values.url as string,
      namespace: (values.namespace as string) || '/smcp',
      office_id: values.office_id as string,
      computer_name: values.computer_name as string,
      headers,
      auto_connect: values.auto_connect as boolean ?? true,
      auto_reconnect: values.auto_reconnect as boolean ?? true,
    };

    await onSubmit(profile, values.api_key as string | undefined);
  };

  return (
    <Form form={form} layout="vertical" initialValues={getInitialValues()} onFinish={handleFinish}>
      <Form.Item name="name" label={t('connection.form.name')} rules={[{ required: true, message: t('connection.form.nameRequired') }]}>
        <Input disabled={!!initialValues} />
      </Form.Item>

      <Form.Item name="url" label={t('connection.form.url')} rules={[{ required: true, message: t('connection.form.urlRequired') }]}>
        <Input placeholder="http://localhost:3000" />
      </Form.Item>

      <Form.Item name="namespace" label={t('connection.form.namespace')}>
        <Input placeholder="/smcp" />
      </Form.Item>

      <Form.Item name="office_id" label={t('connection.form.officeId')} rules={[{ required: true, message: t('connection.form.officeIdRequired') }]}>
        <Input />
      </Form.Item>

      <Form.Item name="computer_name" label={t('connection.form.computerName')} rules={[{ required: true, message: t('connection.form.computerNameRequired') }]}>
        <Input />
      </Form.Item>

      <Form.Item name="api_key" label={t('connection.form.apiKey')}>
        <Input.Password placeholder={initialValues ? t('connection.form.apiKeyUnchanged') : ''} />
      </Form.Item>

      <Card size="small" title={t('connection.form.headers')} style={{ marginBottom: 16 }}>
        <Form.List name="headers">
          {(fields, { add, remove }) => (
            <>
              {fields.map((field) => (
                <Space key={field.key} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
                  <Form.Item name={[field.name, 'key']} noStyle>
                    <Input placeholder="Header-Name" style={{ width: 150 }} />
                  </Form.Item>
                  <Form.Item name={[field.name, 'value']} noStyle>
                    <Input placeholder="value" style={{ width: 200 }} />
                  </Form.Item>
                  <MinusCircleOutlined onClick={() => remove(field.name)} />
                </Space>
              ))}
              <Button type="dashed" onClick={() => add({ key: '', value: '' })} block icon={<PlusOutlined />}>
                {t('connection.form.addHeader')}
              </Button>
            </>
          )}
        </Form.List>
      </Card>

      <Space style={{ marginBottom: 16 }}>
        <Form.Item name="auto_connect" valuePropName="checked" noStyle>
          <Switch />
        </Form.Item>
        <span>{t('connection.form.autoConnect')}</span>

        <Form.Item name="auto_reconnect" valuePropName="checked" noStyle style={{ marginLeft: 24 }}>
          <Switch />
        </Form.Item>
        <span>{t('connection.form.autoReconnect')}</span>
      </Space>

      <Form.Item>
        <Space>
          <Button type="primary" htmlType="submit" loading={loading}>
            {initialValues ? t('common.save') : t('common.add')}
          </Button>
          <Button onClick={onCancel}>{t('common.cancel')}</Button>
        </Space>
      </Form.Item>
    </Form>
  );
}
