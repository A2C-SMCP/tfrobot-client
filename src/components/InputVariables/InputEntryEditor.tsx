import { useState } from 'react';
import { Alert, Button, Select, Space, Switch, Typography } from 'antd';
import { Input } from '@/components/common/Input';
import { useTranslation } from 'react-i18next';
import type { InputDefinition, InputEntry } from '@/stores/inputStore';

interface InputEntryEditorProps {
  entry?: InputEntry;
  fixedKey?: string;
  definition?: InputDefinition;
  initialSecret?: boolean;
  initialValue?: string;
  lockSecret?: boolean;
  showSecretControl?: boolean;
  requireValue?: boolean;
  onSubmit: (key: string, value: string | undefined, secret: boolean) => Promise<void>;
  onCancel: () => void;
}

export function InputEntryEditor({
  entry,
  fixedKey,
  definition,
  initialSecret = false,
  initialValue,
  lockSecret = false,
  showSecretControl = true,
  requireValue = false,
  onSubmit,
  onCancel,
}: InputEntryEditorProps) {
  const { t } = useTranslation();
  const [key, setKey] = useState(fixedKey ?? entry?.key ?? '');
  const [value, setValue] = useState<string | undefined>(
    entry?.value === undefined
      ? (initialValue ?? (definition?.type === 'PickString' ? undefined : ''))
      : String(entry.value),
  );
  const [valueTouched, setValueTouched] = useState(
    entry?.value !== undefined || initialValue !== undefined,
  );
  const [secret, setSecret] = useState(entry?.secret ?? initialSecret);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const validKey = key.length > 0 && key.trim() === key;
  const validValue = definition?.type !== 'PickString'
    || definition.options.some((option) => option.value === value);
  const canSubmit = validKey && validValue && (!requireValue || valueTouched) && !submitting;
  const handleSubmit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(key, entry && !valueTouched ? undefined : (value ?? ''), secret);
    } catch {
      setError(t('inputs.entry.saveFailed'));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      {error && <Alert type="error" showIcon message={error} />}
      <div>
        <Typography.Text>{t('inputs.entry.key')}</Typography.Text>
        <Input
          aria-label={t('inputs.entry.key')}
          value={key}
          disabled={Boolean(fixedKey || entry) || submitting}
          onChange={(event) => setKey(event.target.value)}
          placeholder={t('inputs.entry.keyPlaceholder')}
          status={key.length > 0 && !validKey ? 'error' : undefined}
        />
      </div>
      <div>
        <Typography.Text>{t('inputs.entry.value')}</Typography.Text>
        {definition?.type === 'PickString' ? (
          <Select
            aria-label={t('inputs.entry.value')}
            style={{ width: '100%' }}
            value={value}
            disabled={submitting}
            onChange={(nextValue) => {
              setValue(nextValue);
              setValueTouched(true);
            }}
            placeholder={t('inputs.selectValue')}
            options={definition.options.map((option) => ({
              label: option.label,
              value: option.value,
            }))}
          />
        ) : (
          <Input
            aria-label={t('inputs.entry.value')}
            value={value ?? ''}
            type={secret ? 'password' : 'text'}
            disabled={submitting}
            onChange={(event) => {
              setValue(event.target.value);
              setValueTouched(true);
            }}
            placeholder={entry?.secret && !valueTouched
              ? t('inputs.entry.secretUnchanged')
              : t('inputs.enterValue')}
            onPressEnter={handleSubmit}
          />
        )}
      </div>
      {showSecretControl && (
        <Space align="start">
          <Switch
            aria-label={t('inputs.entry.storeAsSecret')}
            checked={secret}
            disabled={submitting || lockSecret}
            onChange={setSecret}
          />
          <Space direction="vertical" size={0}>
            <Typography.Text>{t('inputs.entry.storeAsSecret')}</Typography.Text>
            <Typography.Text type="secondary">{t('inputs.entry.secretHelp')}</Typography.Text>
          </Space>
        </Space>
      )}
      <Space>
        <Button type="primary" disabled={!canSubmit} loading={submitting} onClick={handleSubmit}>
          {t('common.save')}
        </Button>
        <Button disabled={submitting} onClick={onCancel}>{t('common.cancel')}</Button>
      </Space>
    </Space>
  );
}
