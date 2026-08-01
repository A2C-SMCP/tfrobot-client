import { useEffect } from 'react';
import { App, Button, Form, Input, Space } from 'antd';
import { useTranslation } from 'react-i18next';
import {
  useComputerStore,
  type ComputerFormValues,
  type ComputerInstance,
} from '@/stores/computerStore';

interface GeneralSettingsProps {
  instance: ComputerInstance;
}

export function GeneralSettings({ instance }: GeneralSettingsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [form] = Form.useForm<ComputerFormValues>();
  const loading = useComputerStore((state) => state.loading);
  const updateInstance = useComputerStore((state) => state.updateInstance);

  useEffect(() => {
    form.setFieldsValue({
      name: instance.name,
      description: instance.description,
    });
  }, [form, instance.description, instance.id, instance.name]);

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
    <Form form={form} layout="vertical" onFinish={() => void handleSave()}>
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
      <Space>
        <Button type="primary" htmlType="submit" loading={loading}>
          {t('common.save')}
        </Button>
      </Space>
    </Form>
  );
}
