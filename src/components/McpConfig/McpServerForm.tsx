import { Form, Select, Button, Space, Card, Collapse, Switch } from 'antd';
import { Input } from '@/components/common/Input';
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { McpServerConfig, ToolMeta } from '@/stores/mcpStore';

type ServerType = 'stdio' | 'http' | 'sse';

export type ToolMetaParseResult =
  | { ok: true; value: Record<string, ToolMeta> }
  | { ok: false; error: 'invalid_json' | 'invalid_format' };

export function parseToolMetaJson(json: string | undefined): ToolMetaParseResult {
  if (!json) return { ok: true, value: {} };
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return { ok: false, error: 'invalid_json' };
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return { ok: false, error: 'invalid_format' };
  }
  const result: Record<string, ToolMeta> = {};
  for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return { ok: false, error: 'invalid_format' };
    }
    result[key] = value as ToolMeta;
  }
  return { ok: true, value: result };
}

interface FormValues {
  type: ServerType;
  name: string;
  // Stdio fields
  command?: string;
  args?: string[];
  cwd?: string;
  // Http/Sse fields
  url?: string;
  // Common
  env?: { key: string; value: string }[];
  headers?: { key: string; value: string }[];
  disabled?: boolean;
  // Advanced
  forbidden_tools?: string[];
  default_tool_meta?: { tags?: string[]; auto_apply?: boolean; alias?: string };
  tool_meta_json?: string;
  vrl?: string;
}

interface McpServerFormProps {
  initialValues?: McpServerConfig;
  onSubmit: (config: McpServerConfig) => Promise<void>;
  onCancel: () => void;
  loading?: boolean;
}

export function McpServerForm({ initialValues, onSubmit, onCancel, loading }: McpServerFormProps) {
  const { t } = useTranslation();
  const [form] = Form.useForm<FormValues>();
  const serverType = Form.useWatch('type', form);

  // Convert initial config to form values
  const getInitialFormValues = (): FormValues | undefined => {
    if (!initialValues) return { type: 'stdio', name: '', env: [], args: [] };

    const base = {
      name: initialValues.name,
      disabled: initialValues.disabled,
    };

    const advanced = {
      default_tool_meta: initialValues.default_tool_meta ?? undefined,
      tool_meta_json: initialValues.tool_meta && Object.keys(initialValues.tool_meta).length > 0
        ? JSON.stringify(initialValues.tool_meta, null, 2)
        : undefined,
      vrl: initialValues.vrl ?? undefined,
      forbidden_tools: initialValues.forbidden_tools,
    };

    if (initialValues.type === 'Stdio') {
      const sp = initialValues.server_parameters;
      return {
        ...base,
        ...advanced,
        type: 'stdio',
        command: sp.command,
        args: sp.args,
        cwd: sp.cwd ?? undefined,
        env: Object.entries(sp.env).map(([key, value]) => ({ key, value })),
      };
    }
    if (initialValues.type === 'Http') {
      const sp = initialValues.server_parameters;
      return {
        ...base,
        ...advanced,
        type: 'http',
        url: sp.url,
        headers: Object.entries(sp.headers).map(([key, value]) => ({ key, value })),
      };
    }
    if (initialValues.type === 'Sse') {
      const sp = initialValues.server_parameters;
      return {
        ...base,
        ...advanced,
        type: 'sse',
        url: sp.url,
        headers: Object.entries(sp.headers).map(([key, value]) => ({ key, value })),
      };
    }
    return { type: 'stdio', name: '', env: [], args: [] };
  };

  const handleFinish = async (values: FormValues) => {
    let config: McpServerConfig;

    // Parse and validate tool_meta JSON
    const toolMetaResult = parseToolMetaJson(values.tool_meta_json);
    if (!toolMetaResult.ok) {
      const errorKey = toolMetaResult.error === 'invalid_json'
        ? 'mcp.form.toolMetaInvalidJson'
        : 'mcp.form.toolMetaInvalidFormat';
      form.setFields([{ name: 'tool_meta_json', errors: [t(errorKey)] }]);
      return;
    }
    const toolMeta = toolMetaResult.value;
    const advancedFields = {
      disabled: values.disabled || false,
      forbidden_tools: values.forbidden_tools || [],
      tool_meta: toolMeta,
    };

    const commonFields = {
      name: values.name,
      ...advancedFields,
      default_tool_meta: values.default_tool_meta ?? null,
      vrl: values.vrl ?? null,
    };

    if (values.type === 'stdio') {
      const envObj: Record<string, string> = {};
      values.env?.forEach(({ key, value }) => {
        if (key) envObj[key] = value;
      });

      config = {
        type: 'Stdio' as const,
        ...commonFields,
        server_parameters: {
          command: values.command || '',
          args: values.args || [],
          env: envObj,
          cwd: values.cwd || null,
        },
      };
    } else if (values.type === 'http') {
      const headersObj: Record<string, string> = {};
      values.headers?.forEach(({ key, value }) => {
        if (key) headersObj[key] = value;
      });

      config = {
        type: 'Http' as const,
        ...commonFields,
        server_parameters: {
          url: values.url || '',
          headers: headersObj,
        },
      };
    } else {
      const headersObj: Record<string, string> = {};
      values.headers?.forEach(({ key, value }) => {
        if (key) headersObj[key] = value;
      });

      config = {
        type: 'Sse' as const,
        ...commonFields,
        server_parameters: {
          url: values.url || '',
          headers: headersObj,
        },
      };
    }

    await onSubmit(config);
  };

  return (
    <Form
      form={form}
      layout="vertical"
      initialValues={getInitialFormValues()}
      onFinish={handleFinish}
    >
      <Form.Item
        name="type"
        label={t('mcp.form.type')}
        rules={[{ required: true }]}
      >
        <Select disabled={!!initialValues}>
          <Select.Option value="stdio">Stdio</Select.Option>
          <Select.Option value="http">HTTP</Select.Option>
          <Select.Option value="sse">SSE</Select.Option>
        </Select>
      </Form.Item>

      <Form.Item
        name="name"
        label={t('mcp.form.name')}
        rules={[{ required: true, message: t('mcp.form.nameRequired') }]}
      >
        <Input disabled={!!initialValues} />
      </Form.Item>

      {serverType === 'stdio' && (
        <>
          <Form.Item
            name="command"
            label={t('mcp.form.command')}
            rules={[{ required: true, message: t('mcp.form.commandRequired') }]}
          >
            <Input placeholder="npx, python, node..." />
          </Form.Item>

          <Form.Item label={t('mcp.form.args')}>
            <Form.List name="args">
              {(fields, { add, remove }) => (
                <>
                  {fields.map((field, index) => (
                    <Space key={field.key} style={{ display: 'flex', marginBottom: 8 }}>
                      <Form.Item {...field} noStyle>
                        <Input placeholder={`${t('mcp.form.arg')} ${index + 1}`} />
                      </Form.Item>
                      <MinusCircleOutlined onClick={() => remove(field.name)} />
                    </Space>
                  ))}
                  <Button type="dashed" onClick={() => add('')} block icon={<PlusOutlined />}>
                    {t('mcp.form.addArg')}
                  </Button>
                </>
              )}
            </Form.List>
          </Form.Item>

          <Form.Item name="cwd" label={t('mcp.form.cwd')}>
            <Input placeholder="/path/to/working/directory" />
          </Form.Item>

          <Card size="small" title={t('mcp.form.envVars')} style={{ marginBottom: 16 }}>
            <Form.List name="env">
              {(fields, { add, remove }) => (
                <>
                  {fields.map((field) => (
                    <Space key={field.key} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
                      <Form.Item
                        name={[field.name, 'key']}
                        noStyle
                      >
                        <Input placeholder="KEY" style={{ width: 150 }} />
                      </Form.Item>
                      <Form.Item
                        name={[field.name, 'value']}
                        noStyle
                      >
                        <Input placeholder="value" style={{ width: 200 }} />
                      </Form.Item>
                      <MinusCircleOutlined onClick={() => remove(field.name)} />
                    </Space>
                  ))}
                  <Button type="dashed" onClick={() => add({ key: '', value: '' })} block icon={<PlusOutlined />}>
                    {t('mcp.form.addEnv')}
                  </Button>
                </>
              )}
            </Form.List>
          </Card>
        </>
      )}

      {(serverType === 'http' || serverType === 'sse') && (
        <>
          <Form.Item
            name="url"
            label={t('mcp.form.url')}
            rules={[{ required: true, message: t('mcp.form.urlRequired') }]}
          >
            <Input placeholder="https://..." />
          </Form.Item>

          <Card size="small" title={t('mcp.form.headers')} style={{ marginBottom: 16 }}>
            <Form.List name="headers">
              {(fields, { add, remove }) => (
                <>
                  {fields.map((field) => (
                    <Space key={field.key} style={{ display: 'flex', marginBottom: 8 }} align="baseline">
                      <Form.Item
                        name={[field.name, 'key']}
                        noStyle
                      >
                        <Input placeholder="Header-Name" style={{ width: 150 }} />
                      </Form.Item>
                      <Form.Item
                        name={[field.name, 'value']}
                        noStyle
                      >
                        <Input placeholder="value" style={{ width: 200 }} />
                      </Form.Item>
                      <MinusCircleOutlined onClick={() => remove(field.name)} />
                    </Space>
                  ))}
                  <Button type="dashed" onClick={() => add({ key: '', value: '' })} block icon={<PlusOutlined />}>
                    {t('mcp.form.addHeader')}
                  </Button>
                </>
              )}
            </Form.List>
          </Card>
        </>
      )}

      <Collapse ghost style={{ marginBottom: 16 }}>
        <Collapse.Panel header={t('mcp.form.advancedSettings')} key="advanced">
          <Form.Item name="disabled" label={t('mcp.form.disabled')} valuePropName="checked">
            <Switch />
          </Form.Item>

          <Form.Item name="forbidden_tools" label={t('mcp.form.forbiddenTools')}>
            <Select
              mode="tags"
              placeholder={t('mcp.form.forbiddenToolsPlaceholder')}
              tokenSeparators={[',']}
            />
          </Form.Item>

          <Card size="small" title={t('mcp.form.defaultToolMeta')} style={{ marginBottom: 16 }}>
            <Form.Item name={['default_tool_meta', 'auto_apply']} label={t('mcp.form.defaultToolMetaAutoApply')} valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name={['default_tool_meta', 'alias']} label={t('mcp.form.defaultToolMetaAlias')}>
              <Input />
            </Form.Item>
            <Form.Item name={['default_tool_meta', 'tags']} label={t('mcp.form.defaultToolMetaTags')}>
              <Select
                mode="tags"
                placeholder={t('mcp.form.defaultToolMetaTagsPlaceholder')}
                tokenSeparators={[',']}
              />
            </Form.Item>
          </Card>

          <Form.Item name="tool_meta_json" label={t('mcp.form.toolMeta')}>
            <Input.TextArea
              rows={6}
              style={{ fontFamily: 'monospace' }}
              placeholder='{ "tool_name": { "alias": "...", "tags": [...] } }'
            />
          </Form.Item>

          <Form.Item name="vrl" label={t('mcp.form.vrl')}>
            <Input.TextArea
              rows={6}
              style={{ fontFamily: 'monospace', fontSize: 13 }}
              placeholder="# VRL transformation script"
            />
          </Form.Item>
        </Collapse.Panel>
      </Collapse>

      <Form.Item>
        <Space>
          <Button type="primary" htmlType="submit" loading={loading}>
            {initialValues ? t('common.save') : t('common.add')}
          </Button>
          <Button onClick={onCancel}>
            {t('common.cancel')}
          </Button>
        </Space>
      </Form.Item>
    </Form>
  );
}
