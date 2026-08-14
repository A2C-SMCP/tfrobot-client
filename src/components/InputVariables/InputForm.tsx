import { Form, Select, Button, Space, Switch } from 'antd';
import { Input } from '@/components/common/Input';
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { InputDefinition } from '@/stores/inputStore';

interface InputFormProps {
  initialValues?: InputDefinition;
  onSubmit: (input: InputDefinition) => Promise<void>;
  onCancel: () => void;
  loading?: boolean;
}

export function InputForm({ initialValues, onSubmit, onCancel, loading }: InputFormProps) {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const inputType = Form.useWatch('type', form);
  const passwordMode = Form.useWatch('password', form);
  const pickOptions = Form.useWatch('options', form);

  const getInitialValues = () => {
    if (!initialValues) return { type: 'PromptString' };
    return {
      ...initialValues,
      options: initialValues.type === 'PickString' ? initialValues.options : [],
      args: initialValues.type === 'Command' ? initialValues.args || [] : [],
    };
  };

  const handleFinish = async (values: Record<string, unknown>) => {
    let input: InputDefinition;

    if (values.type === 'PromptString') {
      input = {
        type: 'PromptString',
        id: values.id as string,
        label: (values.label as string | undefined)?.trim() || undefined,
        description: values.description as string | undefined,
        default: values.password
          ? undefined
          : (values.default as string | undefined)?.trim() || undefined,
        password: values.password as boolean | undefined,
      };
    } else if (values.type === 'PickString') {
      input = {
        type: 'PickString',
        id: values.id as string,
        label: (values.label as string | undefined)?.trim() || undefined,
        description: values.description as string | undefined,
        options: (values.options as { label: string; value: string }[]) || [],
        default: (values.default as string | undefined)?.trim() || undefined,
      };
    } else {
      input = {
        type: 'Command',
        id: values.id as string,
        label: (values.label as string | undefined)?.trim() || undefined,
        command: values.command as string,
        args: (values.args as string[])?.filter(Boolean) || undefined,
      };
    }

    await onSubmit(input);
  };

  return (
    <Form form={form} layout="vertical" initialValues={getInitialValues()} onFinish={handleFinish}>
      <Form.Item name="type" label={t('inputs.form.type')} rules={[{ required: true }]}>
        <Select disabled={!!initialValues}>
          <Select.Option value="PromptString">PromptString</Select.Option>
          <Select.Option value="PickString">PickString</Select.Option>
          <Select.Option value="Command">Command</Select.Option>
        </Select>
      </Form.Item>

      <Form.Item name="id" label={t('inputs.form.id')} rules={[{ required: true, message: t('inputs.form.idRequired') }]}>
        <Input disabled={!!initialValues} />
      </Form.Item>

      <Form.Item name="label" label={t('inputs.form.label')}>
        <Input />
      </Form.Item>

      {((inputType === 'PromptString' && !passwordMode) || inputType === 'PickString') && (
        <Form.Item name="default" label={t('inputs.form.defaultValue')}>
          {inputType === 'PickString' ? (
            <Select
              allowClear
              options={((pickOptions || []) as { label?: string; value?: string }[])
                .filter((option) => option.label && option.value)
                .map((option, index) => ({
                  label: option.label,
                  value: option.value,
                  key: `${index}:${option.label}:${option.value}`,
                }))}
            />
          ) : (
            <Input />
          )}
        </Form.Item>
      )}

      {inputType === 'PromptString' && (
        <Form.Item name="password" label={t('inputs.form.password')} valuePropName="checked">
          <Switch />
        </Form.Item>
      )}

      {inputType === 'PickString' && (
          <Form.Item
            label={t('inputs.form.options')}
            required
          >
            <Form.List
              name="options"
              rules={[{
                validator: async (_, options) => {
                  if (!options || options.length === 0) {
                    throw new Error(t('inputs.form.optionRequired'));
                  }
                },
              }]}
            >
            {(fields, { add, remove }, { errors }) => (
              <>
                {fields.map((field) => (
                  <Space key={field.key} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
                    <Form.Item name={[field.name, 'label']} rules={[{ required: true }]}>
                      <Input placeholder="Label" style={{ width: 150 }} />
                    </Form.Item>
                    <Form.Item
                      name={[field.name, 'value']}
                      rules={[{ required: true }]}
                    >
                      <Input placeholder="Value" style={{ width: 150 }} />
                    </Form.Item>
                    <MinusCircleOutlined onClick={() => remove(field.name)} />
                  </Space>
                ))}
                <Button type="dashed" onClick={() => add({ label: '', value: '' })} block icon={<PlusOutlined />}>
                  {t('inputs.form.addOption')}
                </Button>
                <Form.ErrorList errors={errors} />
              </>
            )}
          </Form.List>
        </Form.Item>
      )}

      {inputType === 'Command' && (
        <>
          <Form.Item name="command" label={t('inputs.form.command')} rules={[{ required: true }]}>
            <Input placeholder="echo hello" />
          </Form.Item>
          <Form.Item label={t('inputs.form.args')}>
            <Form.List name="args">
              {(fields, { add, remove }) => (
                <>
                  {fields.map((field, index) => (
                    <Space key={field.key} style={{ display: 'flex', marginBottom: 8 }}>
                      <Form.Item {...field} noStyle>
                        <Input placeholder={`Arg ${index + 1}`} />
                      </Form.Item>
                      <MinusCircleOutlined onClick={() => remove(field.name)} />
                    </Space>
                  ))}
                  <Button type="dashed" onClick={() => add('')} block icon={<PlusOutlined />}>
                    {t('inputs.form.addArg')}
                  </Button>
                </>
              )}
            </Form.List>
          </Form.Item>
        </>
      )}

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
