import { Form, Input, InputNumber, Switch, Select } from 'antd';
import type { FormInstance } from 'antd';

interface SchemaFormProps {
  schema: Record<string, unknown>;
  form: FormInstance;
}

export function SchemaForm({ schema, form }: SchemaFormProps) {
  const properties = (schema.properties || {}) as Record<string, Record<string, unknown>>;
  const required = (schema.required || []) as string[];

  return (
    <Form form={form} layout="vertical" size="small">
      {Object.entries(properties).map(([key, propSchema]) => (
        <SchemaFormItem
          key={key}
          name={key}
          schema={propSchema}
          required={required.includes(key)}
        />
      ))}
    </Form>
  );
}

function SchemaFormItem({
  name,
  schema,
  required,
}: {
  name: string;
  schema: Record<string, unknown>;
  required: boolean;
}) {
  const type = schema.type as string;
  const description = schema.description as string | undefined;
  const enumValues = schema.enum as string[] | undefined;
  const defaultValue = schema.default;

  const rules = required ? [{ required: true, message: `${name} is required` }] : [];

  // Enum -> Select
  if (enumValues) {
    return (
      <Form.Item name={name} label={name} tooltip={description} rules={rules} initialValue={defaultValue}>
        <Select
          options={enumValues.map((v) => ({ label: String(v), value: v }))}
          allowClear
        />
      </Form.Item>
    );
  }

  switch (type) {
    case 'string':
      return (
        <Form.Item name={name} label={name} tooltip={description} rules={rules} initialValue={defaultValue}>
          <Input />
        </Form.Item>
      );
    case 'number':
    case 'integer':
      return (
        <Form.Item name={name} label={name} tooltip={description} rules={rules} initialValue={defaultValue}>
          <InputNumber style={{ width: '100%' }} />
        </Form.Item>
      );
    case 'boolean':
      return (
        <Form.Item name={name} label={name} tooltip={description} valuePropName="checked" initialValue={defaultValue ?? false}>
          <Switch />
        </Form.Item>
      );
    case 'object':
      // For nested objects, fall back to JSON text input
      return (
        <Form.Item name={name} label={name} tooltip={description} rules={rules}>
          <Input.TextArea rows={3} placeholder='{"key": "value"}' />
        </Form.Item>
      );
    case 'array':
      return (
        <Form.Item name={name} label={name} tooltip={description} rules={rules}>
          <Input.TextArea rows={2} placeholder='["item1", "item2"]' />
        </Form.Item>
      );
    default:
      return (
        <Form.Item name={name} label={name} tooltip={description} rules={rules} initialValue={defaultValue}>
          <Input />
        </Form.Item>
      );
  }
}
