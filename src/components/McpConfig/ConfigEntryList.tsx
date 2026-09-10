import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { Button, Form, Space, Tag, Typography } from 'antd';
import { DeleteOutlined, EditOutlined, PlusOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { InputDefinition } from '@/stores/inputStore';
import { ConfigValueEditor } from './ConfigValueEditor';
import {
  applyConfigEntryEdit,
  draftInputDefinitions,
  type ConfigEntryFormValue,
} from './configValue';

interface ConfigEntryListProps {
  name: 'env' | 'headers';
  draftPrefix: string;
  instanceId: string;
  inputs: InputDefinition[];
  addLabel: string;
  onEntriesChange: (name: 'env' | 'headers', entries: ConfigEntryFormValue[]) => void;
}

function entryType(entry: ConfigEntryFormValue | undefined): string {
  if (!entry) return 'Constant';
  if (entry.value.type === 'Constant') return 'Constant';
  return entry.value.definition?.type ?? 'Unresolved';
}

function entrySummary(entry: ConfigEntryFormValue | undefined, unresolved: string): string {
  if (!entry) return '';
  if (entry.value.type === 'Constant') return entry.value.value;
  if (!entry.value.definition) return unresolved;
  const definition = entry.value.definition;
  if (definition.type === 'PickString') return `${definition.id} · ${definition.options.length}`;
  if (definition.type === 'Command') return `${definition.id} · ${definition.command}`;
  return definition.password ? `${definition.id} · Secret` : definition.id;
}

export function ConfigEntryList({ name, instanceId, inputs, addLabel, draftPrefix, onEntriesChange }: ConfigEntryListProps) {
  const { t } = useTranslation();
  const form = Form.useFormInstance();
  const entries = (Form.useWatch(name, form) ?? []) as ConfigEntryFormValue[];
  const [editor, setEditor] = useNavigationState<{ index?: number; value?: ConfigEntryFormValue } | null>(`${draftPrefix}entry-editor.${name}`, null);

  return (
    <Form.List name={name}>
      {(fields, { remove }) => (
        <>
          <Space direction="vertical" style={{ width: '100%' }} size={8}>
            {fields.map((field) => {
              const entry = entries[field.name];
              const type = entryType(entry);
              return (
                <div
                  key={field.key}
                  style={{
                    alignItems: 'center',
                    border: '1px solid var(--ant-color-border-secondary, #f0f0f0)',
                    borderRadius: 8,
                    display: 'grid',
                    gap: 10,
                    gridTemplateColumns: 'minmax(130px, 1fr) auto minmax(130px, 1.5fr) auto',
                    padding: '10px 12px',
                  }}
                >
                  <Typography.Text ellipsis title={entry?.key}>{entry?.key}</Typography.Text>
                  <Tag color={type === 'Constant' ? 'default' : type === 'Unresolved' ? 'red' : 'blue'}>
                    {type}
                  </Tag>
                  <Typography.Text
                    type="secondary"
                    ellipsis
                    title={entrySummary(entry, t('mcp.form.unresolvedInput'))}
                  >
                    {entrySummary(entry, t('mcp.form.unresolvedInput'))}
                  </Typography.Text>
                  <Space size={2}>
                    <Button
                      type="text"
                      icon={<EditOutlined />}
                      aria-label={t('mcp.form.editConfigItemFor', { key: entry?.key })}
                      onClick={() => setEditor({ index: field.name, value: entry })}
                    />
                    <Button
                      type="text"
                      danger
                      icon={<DeleteOutlined />}
                      aria-label={t('mcp.form.removeConfigItemFor', { key: entry?.key })}
                      onClick={() => remove(field.name)}
                    />
                  </Space>
                </div>
              );
            })}
          </Space>
          <Button
            type="dashed"
            onClick={() => setEditor({})}
            block
            icon={<PlusOutlined />}
            style={{ marginTop: 8 }}
          >
            {addLabel}
          </Button>
          <Typography.Text type="secondary">
            {t('mcp.form.constantHint')}
          </Typography.Text>
          <Form.Item noStyle shouldUpdate>
            {() => <Form.ErrorList errors={form.getFieldError(name)} />}
          </Form.Item>
          {editor && (
            <ConfigValueEditor
              draftPrefix={`${draftPrefix}${name}.`}
              open
              instanceId={instanceId}
              initialValue={editor.value}
              inputs={draftInputDefinitions(entries, inputs)}
              existingKeys={entries
                .filter((_, index) => index !== editor.index)
                .map((entry) => entry.key)}
              onSubmit={(entry) => {
                onEntriesChange(name, applyConfigEntryEdit(entries, editor.index, entry));
                setEditor(null);
              }}
              onCancel={() => setEditor(null)}
            />
          )}
        </>
      )}
    </Form.List>
  );
}
