import { useState } from 'react';
import { Alert, Button, Space, Switch, Typography } from 'antd';
import { Input } from '@/components/common/Input';
import { useTranslation } from 'react-i18next';
import type { InputEntry } from '@/stores/inputStore';

interface InputEntryEditorProps {
  entry?: InputEntry;
  onSubmit: (key: string, value: string | undefined, secret: boolean) => Promise<void>;
  onCancel: () => void;
}

export function InputEntryEditor({ entry, onSubmit, onCancel }: InputEntryEditorProps) {
  const { t } = useTranslation();
  const [key, setKey] = useState(entry?.key ?? '');
  const [value, setValue] = useState(entry?.value === undefined ? '' : String(entry.value));
  const [valueTouched, setValueTouched] = useState(entry?.value !== undefined);
  const [secret, setSecret] = useState(entry?.secret ?? false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const validKey = key.length > 0 && key.trim() === key;
  const handleSubmit = async () => {
    if (!validKey || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(key, entry && !valueTouched ? undefined : value, secret);
    } catch (cause) {
      setError(String(cause));
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
          value={key}
          disabled={Boolean(entry) || submitting}
          onChange={(event) => setKey(event.target.value)}
          placeholder={t('inputs.entry.keyPlaceholder')}
          status={key.length > 0 && !validKey ? 'error' : undefined}
        />
      </div>
      <div>
        <Typography.Text>{t('inputs.entry.value')}</Typography.Text>
        <Input
          value={value}
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
      </div>
      <Space>
        <Switch checked={secret} disabled={submitting} onChange={setSecret} />
        <Typography.Text>{t('inputs.entry.storeAsSecret')}</Typography.Text>
      </Space>
      <Space>
        <Button type="primary" disabled={!validKey} loading={submitting} onClick={handleSubmit}>
          {t('common.save')}
        </Button>
        <Button disabled={submitting} onClick={onCancel}>{t('common.cancel')}</Button>
      </Space>
    </Space>
  );
}
