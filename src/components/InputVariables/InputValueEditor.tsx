import { useState } from 'react';
import { Button, Select, Space } from 'antd';
import { Input } from '@/components/common/Input';
import { useTranslation } from 'react-i18next';
import type { InputDefinition } from '@/stores/inputStore';

interface InputValueEditorProps {
  inputId: string;
  inputs: InputDefinition[];
  currentValue: unknown;
  onSubmit: (value: string) => Promise<void>;
  onCancel: () => void;
}

export function InputValueEditor({ inputId, inputs, currentValue, onSubmit, onCancel }: InputValueEditorProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState(currentValue !== undefined ? String(currentValue) : '');
  const [submitting, setSubmitting] = useState(false);

  const input = inputs.find((i) => i.id === inputId);

  const handleSubmit = async () => {
    setSubmitting(true);
    try {
      await onSubmit(value);
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
          value={value || undefined}
          onChange={(v) => setValue(v)}
          placeholder={t('inputs.selectValue')}
        >
          {input.options.map((opt) => (
            <Select.Option key={opt.value} value={opt.value}>
              {opt.label}
            </Select.Option>
          ))}
        </Select>
        <Space>
          <Button type="primary" onClick={handleSubmit} loading={submitting}>{t('common.save')}</Button>
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
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder={t('inputs.enterValue')}
        type={input?.type === 'PromptString' && input.password ? 'password' : 'text'}
        onPressEnter={handleSubmit}
      />
      <Space>
        <Button type="primary" onClick={handleSubmit} loading={submitting}>{t('common.save')}</Button>
        <Button onClick={onCancel}>{t('common.cancel')}</Button>
      </Space>
    </div>
  );
}
