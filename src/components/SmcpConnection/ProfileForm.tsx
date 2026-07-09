import { Form, Button, Space, Card, Checkbox } from 'antd';
import { Input } from '@/components/common/Input';
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { ManualSmcpApiKeyAction, ManualSmcpTarget } from '@/stores/connectionTargetStore';

interface ProfileFormProps {
  initialValues?: ManualSmcpTarget;
  onSubmit: (target: ManualSmcpTarget, apiKeyAction: ManualSmcpApiKeyAction) => Promise<void>;
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

    const target: ManualSmcpTarget = {
      id: initialValues?.id ?? '',
      name: values.name as string,
      url: values.url as string,
      namespace: (values.namespace as string) || '/smcp',
      office_id: values.office_id as string,
      headers,
    };
    const apiKey = String(values.api_key ?? '').trim();
    const apiKeyAction: ManualSmcpApiKeyAction = values.clear_api_key
      ? { kind: 'clear' }
      : apiKey
        ? { kind: 'set', value: apiKey }
        : { kind: 'unchanged' };

    await onSubmit(target, apiKeyAction);
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

      <Form.Item name="api_key" label={t('connection.form.apiKey')}>
        <Input.Password placeholder={initialValues ? t('connection.form.apiKeyUnchanged') : ''} />
      </Form.Item>
      {initialValues && (
        <Form.Item name="clear_api_key" valuePropName="checked">
          <Checkbox>{t('connection.form.clearApiKey')}</Checkbox>
        </Form.Item>
      )}

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
