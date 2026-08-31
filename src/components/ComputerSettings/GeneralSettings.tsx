import { useEffect } from 'react';
import { Alert, App, Button, Form, Input, InputNumber, Space } from 'antd';
import { useTranslation } from 'react-i18next';
import {
  useComputerStore,
  type ComputerFormValues,
  type ComputerInstance,
} from '@/stores/computerStore';

interface GeneralSettingsProps {
  instance: ComputerInstance;
}

interface GeneralSettingsValues extends ComputerFormValues {
  mcpStartConcurrency: number;
}

export function GeneralSettings({ instance }: GeneralSettingsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [form] = Form.useForm<GeneralSettingsValues>();
  const loading = useComputerStore((state) => state.loading);
  const updateInstance = useComputerStore((state) => state.updateInstance);

  useEffect(() => {
    form.setFieldsValue({
      name: instance.name,
      description: instance.description,
      mcpStartConcurrency: instance.mcpStartConcurrency ?? 5,
    });
  }, [form, instance.description, instance.id, instance.mcpStartConcurrency, instance.name]);

  const handleSave = async () => {
    const values = await form.validateFields();
    try {
      await updateInstance(instance.id, values);
      message.success(t('common.saved'));
    } catch (error) {
      message.error(String(error));
    }
  };

  return (
    <Form
      form={form}
      layout="vertical"
      initialValues={{
        name: instance.name,
        description: instance.description,
        mcpStartConcurrency: instance.mcpStartConcurrency ?? 5,
      }}
      onFinish={() => void handleSave()}
    >
      <Form.Item
        name="name"
        label={t('computer.form.name')}
        rules={[
          {
            validator: (_, value) => value?.trim()
              ? Promise.resolve()
              : Promise.reject(new Error(t('computer.form.nameRequired'))),
          },
        ]}
      >
        <Input autoComplete="off" />
      </Form.Item>
      <Form.Item name="description" label={t('computer.form.description')}>
        <Input.TextArea rows={4} />
      </Form.Item>
      <Form.Item
        name="mcpStartConcurrency"
        label={t('computer.form.mcpStartConcurrency')}
        extra={t('computer.form.mcpStartConcurrencyHint')}
        rules={[{ required: true, type: 'number', min: 1, max: 64 }]}
      >
        <InputNumber min={1} max={64} precision={0} />
      </Form.Item>
      <Alert type="info" showIcon message={t('computer.form.mcpStartConcurrencyRunningHint')} />
      <Space>
        <Button type="primary" htmlType="submit" loading={loading}>
          {t('common.save')}
        </Button>
      </Space>
    </Form>
  );
}
