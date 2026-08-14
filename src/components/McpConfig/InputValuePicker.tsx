import { Select } from 'antd';
import { useTranslation } from 'react-i18next';
import type { InputDefinition } from '@/stores/inputStore';

interface InputValuePickerProps {
  inputs: InputDefinition[];
  onSelect: (value: string) => void;
}

export function InputValuePicker({ inputs, onSelect }: InputValuePickerProps) {
  const { t } = useTranslation();

  return (
    <Select
      aria-label={t('mcp.form.inputReference')}
      placeholder={t('mcp.form.inputReference')}
      style={{ width: 180 }}
      value={undefined}
      options={inputs.map((input) => ({
        label: `${input.label || (input.type === 'Command' ? undefined : input.description) || input.id} (${input.id})`,
        value: input.id,
      }))}
      onChange={(inputId) => onSelect(`\${input:${inputId}}`)}
    />
  );
}
