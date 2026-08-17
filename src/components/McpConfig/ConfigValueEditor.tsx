import { Select, Space } from 'antd';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/common/Input';
import type { InputDefinition } from '@/stores/inputStore';
import type { ConfigValueFormValue } from './configValue';

interface ConfigValueEditorProps {
  inputs: InputDefinition[];
  value?: ConfigValueFormValue;
  onChange?: (value: ConfigValueFormValue) => void;
}

export function ConfigValueEditor({ inputs, value, onChange }: ConfigValueEditorProps) {
  const { t } = useTranslation();
  const current = value ?? { source: 'constant', value: '' };

  const setSource = (source: ConfigValueFormValue['source']) => {
    if (source === current.source) return;
    onChange?.({ source, value: '' });
  };

  return (
    <Space.Compact>
      <Select
        aria-label={t('mcp.form.valueSource')}
        style={{ width: 120 }}
        value={current.source}
        options={[
          { label: t('mcp.form.constantSource'), value: 'constant' },
          { label: t('mcp.form.inputSource'), value: 'input' },
        ]}
        onChange={setSource}
      />
      {current.source === 'constant' ? (
        <Input
          aria-label={t('mcp.form.constantValue')}
          placeholder={t('mcp.form.constantValue')}
          style={{ width: 240 }}
          value={current.value}
          onChange={(event) => onChange?.({ source: 'constant', value: event.target.value })}
        />
      ) : (
        <Select
          aria-label={t('mcp.form.inputReference')}
          placeholder={t('mcp.form.inputReference')}
          style={{ width: 240 }}
          value={current.value || undefined}
          options={inputs.map((input) => ({
            label: `${input.label || (input.type === 'Command' ? undefined : input.description) || input.id} (${input.id})`,
            value: input.id,
          }))}
          onChange={(inputId) => onChange?.({ source: 'input', value: inputId })}
        />
      )}
    </Space.Compact>
  );
}
