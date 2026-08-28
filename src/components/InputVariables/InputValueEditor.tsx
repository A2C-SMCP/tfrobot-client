import { useState } from 'react';
import { Button, Select, Space } from 'antd';
import { Input } from '@/components/common/Input';
import { useTranslation } from 'react-i18next';
import type { InputDefinition } from '@/stores/inputStore';

interface InputValueEditorProps {
  inputId: string;
  inputs: InputDefinition[];
  currentValue: unknown;
  disabled?: boolean;
  onSubmit: (value: string) => Promise<void>;
  onCancel: () => void;
}

export function InputValueEditor({ inputId, inputs, currentValue, disabled = false, onSubmit, onCancel }: InputValueEditorProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState<string | undefined>(currentValue !== undefined ? String(currentValue) : undefined);
  const [submitting, setSubmitting] = useState(false);

  const input = inputs.find((i) => i.id === inputId);
  const validPickSelection = input?.type !== 'PickString'
    || input.options.some((option) => option.value === value);
  const canSubmit = !disabled && !submitting && validPickSelection;

  const handleSubmit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onSubmit(value ?? '');
    } finally {
      setSubmitting(false);
    }
  };

  // PickString: render as Select
  if (input?.type === 'PickString') {
    return (
      <div>
        <Select
          style={{ width: '100%', marginBottom: 16 }}
          value={value}
          onChange={(v) => setValue(v)}
          disabled={disabled || submitting}
          placeholder={t('inputs.selectValue')}
        >
          {input.options.map((opt, index) => (
            <Select.Option key={`${index}:${opt.label}:${opt.value}`} value={opt.value}>
              {opt.label}
            </Select.Option>
          ))}
        </Select>
        <Space>
          <Button type="primary" onClick={handleSubmit} loading={submitting} disabled={!canSubmit}>{t('common.save')}</Button>
          <Button onClick={onCancel}>{t('common.cancel')}</Button>
        </Space>
      </div>
    );
  }

  // PromptString or Command: render as Input
  return (
    <div>
      <Input
        style={{ marginBottom: 16 }}
        value={value ?? ''}
        onChange={(e) => setValue(e.target.value)}
        disabled={disabled || submitting}
        placeholder={t('inputs.enterValue')}
        type={input?.type === 'PromptString' && input.password ? 'password' : 'text'}
        onPressEnter={handleSubmit}
      />
      <Space>
        <Button type="primary" onClick={handleSubmit} loading={submitting} disabled={!canSubmit}>{t('common.save')}</Button>
        <Button onClick={onCancel}>{t('common.cancel')}</Button>
      </Space>
    </div>
  );
}
