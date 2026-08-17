import { useEffect } from 'react';
import { Alert, AutoComplete, Button, Form, Modal, Select, Space, Switch } from 'antd';
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/common/Input';
import type { InputDefinition } from '@/stores/inputStore';
import type { ConfigEntryFormValue } from './configValue';

type ConfigEntryType = 'Constant' | InputDefinition['type'];

interface EditorValues {
  key: string;
  type: ConfigEntryType;
  value?: string;
  id?: string;
  label?: string;
  description?: string;
  default?: string;
  password?: boolean;
  options?: { label: string; value: string }[];
  command?: string;
  args?: string[];
}

interface ConfigValueEditorProps {
  open: boolean;
  initialValue?: ConfigEntryFormValue;
  inputs: InputDefinition[];
  existingKeys: string[];
  onSubmit: (value: ConfigEntryFormValue) => void;
  onCancel: () => void;
}

function definitionToValues(definition: InputDefinition): Partial<EditorValues> {
  return {
    ...definition,
    type: definition.type,
    label: undefined,
    description: 'description' in definition
      ? definition.description ?? definition.label
      : definition.label,
    options: definition.type === 'PickString' ? definition.options : [],
    args: definition.type === 'Command' ? definition.args ?? [] : [],
  };
}

function entryToValues(entry: ConfigEntryFormValue | undefined): EditorValues {
  if (!entry) return { key: '', type: 'Constant', value: '' };
  if (entry.value.type === 'Constant') {
    return { key: entry.key, type: 'Constant', value: entry.value.value };
  }
  const definition = entry.value.definition;
  return {
    key: entry.key,
    type: definition?.type ?? 'PromptString',
    id: entry.value.inputId,
    ...(definition ? definitionToValues(definition) : {}),
  };
}

function valuesToDefinition(values: EditorValues): InputDefinition {
  const id = values.id?.trim() ?? '';
  const description = values.description?.trim() || undefined;
  if (values.type === 'PromptString') {
    return {
      type: 'PromptString',
      id,
      description,
      default: values.password ? undefined : values.default?.trim() || undefined,
      password: values.password || undefined,
    };
  }
  if (values.type === 'PickString') {
    return {
      type: 'PickString',
      id,
      description,
      options: values.options ?? [],
      default: values.default?.trim() || undefined,
    };
  }
  return {
    type: 'Command',
    id,
    label: description,
    command: values.command?.trim() ?? '',
    args: values.args?.filter(Boolean),
  };
}

export function ConfigValueEditor({
  open,
  initialValue,
  inputs,
  existingKeys,
  onSubmit,
  onCancel,
}: ConfigValueEditorProps) {
  const { t } = useTranslation();
  const [form] = Form.useForm<EditorValues>();
  const type = Form.useWatch('type', form);
  const password = Form.useWatch('password', form);
  const pickOptions = Form.useWatch('options', form);
  const inputId = Form.useWatch('id', form);
  const matchingInput = inputs.find((input) => input.id === inputId);

  useEffect(() => {
    if (open) {
      form.resetFields();
      form.setFieldsValue(entryToValues(initialValue));
    }
  }, [form, initialValue, open]);

  useEffect(() => {
    if (type !== 'PickString') return;
    const currentDefault = form.getFieldValue('default');
    if (currentDefault && !(pickOptions ?? []).some((option) => option.value === currentDefault)) {
      form.setFieldValue('default', undefined);
    }
  }, [form, pickOptions, type]);

  const loadDefinition = (id: string) => {
    const definition = inputs.find((input) => input.id === id);
    if (definition) form.setFieldsValue(definitionToValues(definition));
  };

  const handleFinish = (values: EditorValues) => {
    const key = values.key.trim();
    if (values.type === 'Constant') {
      onSubmit({ key, value: { type: 'Constant', value: values.value ?? '' } });
      return;
    }
    const definition = valuesToDefinition(values);
    onSubmit({
      key,
      value: { type: 'Input', inputId: definition.id, definition },
    });
  };

  return (
    <Modal
      title={initialValue ? t('mcp.form.editConfigItem') : t('mcp.form.addConfigItem')}
      open={open}
      onCancel={onCancel}
      footer={null}
      destroyOnHidden
      width={620}
    >
      <Form
        name="mcp-config-entry"
        form={form}
        layout="vertical"
        initialValues={entryToValues(initialValue)}
        onFinish={handleFinish}
      >
        <Form.Item
          name="key"
          label={t('mcp.form.configKey')}
          rules={[
            { required: true, whitespace: true, message: t('mcp.form.configKeyRequired') },
            {
              validator: async (_, value: string | undefined) => {
                if (value?.trim() && existingKeys.includes(value.trim())) {
                  throw new Error(t('mcp.form.configKeyDuplicate'));
                }
              },
            },
          ]}
        >
          <Input placeholder="KEY" />
        </Form.Item>

        <Form.Item name="type" label={t('mcp.form.configItemType')} rules={[{ required: true }]}>
          <Select
            virtual={false}
            options={[
              { value: 'Constant', label: t('mcp.form.constantSource') },
              { value: 'PromptString', label: 'PromptString' },
              { value: 'PickString', label: 'PickString' },
              { value: 'Command', label: 'Command' },
            ]}
          />
        </Form.Item>

        {type === 'Constant' ? (
          <>
            <Form.Item name="value" label={t('mcp.form.constantValue')}>
              <Input.TextArea rows={3} />
            </Form.Item>
            <Alert type="warning" showIcon message={t('mcp.form.constantHint')} />
          </>
        ) : (
          <>
            <Form.Item
              name="id"
              label={t('inputs.form.id')}
              rules={[{ required: true, whitespace: true, message: t('inputs.form.idRequired') }]}
            >
              <AutoComplete
                options={inputs.map((input) => ({ value: input.id }))}
                onSelect={loadDefinition}
                onChange={(id) => loadDefinition(id)}
                placeholder={t('mcp.form.inputIdPlaceholder')}
              />
            </Form.Item>
            {matchingInput && (
              <Alert
                type="info"
                showIcon
                message={t('mcp.form.sharedInputHint', { id: matchingInput.id })}
                style={{ marginBottom: 16 }}
              />
            )}
            <Form.Item name="description" label={t('inputs.form.description')}>
              <Input />
            </Form.Item>
            {((type === 'PromptString' && !password) || type === 'PickString') && (
              <Form.Item name="default" label={t('inputs.form.defaultValue')}>
                {type === 'PickString' ? (
                  <Select
                    allowClear
                    options={(pickOptions ?? [])
                      .filter((option) => option.label && option.value)
                      .map((option, index) => ({
                        label: option.label,
                        value: option.value,
                        key: `${index}:${option.label}:${option.value}`,
                      }))}
                  />
                ) : <Input />}
              </Form.Item>
            )}
            {type === 'PromptString' && (
              <Form.Item name="password" label={t('inputs.form.password')} valuePropName="checked">
                <Switch />
              </Form.Item>
            )}
            {type === 'PickString' && (
              <Form.Item label={t('inputs.form.options')} required>
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
                            <Input placeholder="Label" style={{ width: 180 }} />
                          </Form.Item>
                          <Form.Item name={[field.name, 'value']} rules={[{ required: true }]}>
                            <Input placeholder="Value" style={{ width: 180 }} />
                          </Form.Item>
                          <MinusCircleOutlined onClick={() => remove(field.name)} />
                        </Space>
                      ))}
                      <Button
                        type="dashed"
                        onClick={() => add({ label: '', value: '' })}
                        block
                        icon={<PlusOutlined />}
                      >
                        {t('inputs.form.addOption')}
                      </Button>
                      <Form.ErrorList errors={errors} />
                    </>
                  )}
                </Form.List>
              </Form.Item>
            )}
            {type === 'Command' && (
              <>
                <Form.Item name="command" label={t('inputs.form.command')} rules={[{ required: true }]}>
                  <Input />
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
          </>
        )}

        <Form.Item style={{ marginTop: 20, marginBottom: 0 }}>
          <Space>
            <Button
              type="primary"
              onClick={() => form.submit()}
            >
              {t('common.confirm')}
            </Button>
            <Button onClick={onCancel}>{t('common.cancel')}</Button>
          </Space>
        </Form.Item>
      </Form>
    </Modal>
  );
}
