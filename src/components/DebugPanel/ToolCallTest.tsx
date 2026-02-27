import { useState } from 'react';
import { Button, Form, InputNumber, Space, Segmented, Alert, Tag, Typography, Image } from 'antd';
import { PlayCircleOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useDebugStore, type ToolInfo, type ToolCallResponse } from '@/stores/debugStore';
import { SchemaForm } from './SchemaForm';

const { Text } = Typography;

export function ToolCallTest({ tool }: { tool: ToolInfo }) {
  const { t } = useTranslation();
  const { executeTool, lastCallResult, calling } = useDebugStore();
  const [form] = Form.useForm();
  const [mode, setMode] = useState<'form' | 'json'>('form');
  const [jsonText, setJsonText] = useState('{}');
  const [timeout, setTimeout] = useState<number | undefined>();

  const handleExecute = async () => {
    let params: Record<string, unknown>;

    if (mode === 'form') {
      try {
        const values = await form.validateFields();
        // Parse JSON strings for object/array fields
        params = {};
        for (const [key, value] of Object.entries(values)) {
          if (typeof value === 'string') {
            try {
              const parsed = JSON.parse(value);
              if (typeof parsed === 'object') {
                params[key] = parsed;
                continue;
              }
            } catch {
              // not JSON, use as-is
            }
          }
          if (value !== undefined && value !== null && value !== '') {
            params[key] = value;
          }
        }
      } catch {
        return; // validation failed
      }
    } else {
      try {
        params = JSON.parse(jsonText);
      } catch {
        return;
      }
    }

    await executeTool(tool.name, params, timeout);
  };

  return (
    <div>
      <Space style={{ marginBottom: 12 }}>
        <Segmented
          options={[
            { label: t('debug.formMode'), value: 'form' },
            { label: t('debug.jsonMode'), value: 'json' },
          ]}
          value={mode}
          onChange={(v) => setMode(v as 'form' | 'json')}
        />
        <InputNumber
          placeholder={t('debug.timeout')}
          value={timeout}
          onChange={(v) => setTimeout(v ?? undefined)}
          min={1}
          max={300}
          addonAfter="s"
          style={{ width: 160 }}
        />
        <Button type="primary" icon={<PlayCircleOutlined />} onClick={handleExecute} loading={calling}>
          {t('debug.execute')}
        </Button>
      </Space>

      {mode === 'form' ? (
        <SchemaForm schema={tool.inputSchema} form={form} />
      ) : (
        <Form.Item>
          <textarea
            value={jsonText}
            onChange={(e) => setJsonText(e.target.value)}
            style={{
              width: '100%',
              minHeight: 120,
              fontFamily: 'monospace',
              fontSize: 12,
              padding: 8,
              border: '1px solid #d9d9d9',
              borderRadius: 6,
            }}
          />
        </Form.Item>
      )}

      {lastCallResult && <ToolCallResultView response={lastCallResult} />}
    </div>
  );
}

function ToolCallResultView({ response }: { response: ToolCallResponse }) {
  const { t } = useTranslation();

  return (
    <div style={{ marginTop: 16 }}>
      <Space style={{ marginBottom: 8 }}>
        <Tag color={response.success ? 'success' : 'error'}>
          {response.success ? t('common.success') : t('common.error')}
        </Tag>
        <Text type="secondary">{response.duration_ms}ms</Text>
      </Space>

      {response.error && (
        <Alert type="error" message={response.error} style={{ marginBottom: 8 }} />
      )}

      {response.result?.content.map((item, i) => {
        if (item.type === 'text') {
          return (
            <pre
              key={i}
              style={{
                background: '#f5f5f5',
                padding: 12,
                borderRadius: 6,
                fontSize: 12,
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
              }}
            >
              {item.text}
            </pre>
          );
        }
        if (item.type === 'image' && item.data) {
          return (
            <Image
              key={i}
              src={`data:${item.mime_type || 'image/png'};base64,${item.data}`}
              style={{ maxWidth: '100%', marginBottom: 8 }}
            />
          );
        }
        if (item.type === 'resource') {
          return <Tag key={i}>{item.uri}</Tag>;
        }
        return null;
      })}
    </div>
  );
}
