import { Form, Input, Select, Button, Space, Card } from 'antd';
import { MinusCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { McpServerConfig, StdioServerConfig, HttpServerConfig, SseServerConfig } from '@/stores/mcpStore';

type ServerType = 'stdio' | 'http' | 'sse';

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

    if ('Stdio' in initialValues) {
      const cfg = initialValues.Stdio;
      return {
        type: 'stdio',
        name: cfg.name,
        command: cfg.command,
        args: cfg.args,
        cwd: cfg.cwd,
        env: Object.entries(cfg.env).map(([key, value]) => ({ key, value })),
        disabled: cfg.disabled,
      };
    }
    if ('Http' in initialValues) {
      const cfg = initialValues.Http;
      return {
        type: 'http',
        name: cfg.name,
        url: cfg.url,
        headers: Object.entries(cfg.headers).map(([key, value]) => ({ key, value })),
        disabled: cfg.disabled,
      };
    }
    if ('Sse' in initialValues) {
      const cfg = initialValues.Sse;
      return {
        type: 'sse',
        name: cfg.name,
        url: cfg.url,
        headers: Object.entries(cfg.headers).map(([key, value]) => ({ key, value })),
        disabled: cfg.disabled,
      };
    }
    return { type: 'stdio', name: '', env: [], args: [] };
  };

  const handleFinish = async (values: FormValues) => {
    let config: McpServerConfig;

    if (values.type === 'stdio') {
      const envObj: Record<string, string> = {};
      values.env?.forEach(({ key, value }) => {
        if (key) envObj[key] = value;
      });

      const stdioConfig: StdioServerConfig = {
        name: values.name,
        command: values.command || '',
        args: values.args || [],
        env: envObj,
        cwd: values.cwd,
        disabled: values.disabled || false,
        forbidden_tools: [],
        tool_meta: {},
      };
      config = { Stdio: stdioConfig };
    } else if (values.type === 'http') {
      const headersObj: Record<string, string> = {};
      values.headers?.forEach(({ key, value }) => {
        if (key) headersObj[key] = value;
      });

      const httpConfig: HttpServerConfig = {
        name: values.name,
        url: values.url || '',
        headers: headersObj,
        disabled: values.disabled || false,
        forbidden_tools: [],
        tool_meta: {},
      };
      config = { Http: httpConfig };
    } else {
      const headersObj: Record<string, string> = {};
      values.headers?.forEach(({ key, value }) => {
        if (key) headersObj[key] = value;
      });

      const sseConfig: SseServerConfig = {
        name: values.name,
        url: values.url || '',
        headers: headersObj,
        disabled: values.disabled || false,
        forbidden_tools: [],
        tool_meta: {},
      };
      config = { Sse: sseConfig };
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
