import { ConfigSaveStatus } from '@/components/ComputerSettings/ConfigSaveStatus';
import { useNavigationState, useForgetNavigationState } from '@/components/Navigation/navigationMemoryState';
import { useConfigCommit } from '@/components/ComputerSettings/useConfigCommit';
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
  const prefix = `input.${inputId}.`;
  const [value, setValue] = useNavigationState<string | undefined>(`${prefix}editValue`, currentValue !== undefined ? String(currentValue) : undefined);
  const submission = useConfigCommit(`${prefix}valueCommit`, (value: string) => onSubmit(value));
  const submitting = submission.state === 'saving';
  const forget = useForgetNavigationState();
  const edit = (next: string) => { setValue(next); submission.dirty(); };
  const cancel = () => { forget(prefix); onCancel(); };

  const input = inputs.find((i) => i.id === inputId);
  const validPickSelection = input?.type !== 'PickString'
    || input.options.some((option) => option.value === value);
  const canSubmit = !disabled && !submitting && validPickSelection;

  const handleSubmit = async () => {
    if (!canSubmit) return;
    await submission.commit(value ?? '');

  };

  // PickString: render as Select
  if (input?.type === 'PickString') {
    return (
      <div>
        <Select
          style={{ width: '100%', marginBottom: 16 }}
          value={value}
          onChange={edit}
          disabled={disabled || submitting}
          placeholder={t('inputs.selectValue')}
        >
          {input.options.map((opt, index) => (
            <Select.Option key={`${index}:${opt.label}:${opt.value}`} value={opt.value}>
              {opt.label}
            </Select.Option>
          ))}
        </Select>
        <ConfigSaveStatus state={submission.state} />
      <Space>
          <Button type="primary" onClick={handleSubmit} loading={submitting} disabled={!canSubmit}>{t('common.save')}</Button>
          <Button disabled={submitting} onClick={cancel}>{t('common.cancel')}</Button>
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
        onChange={(e) => edit(e.target.value)}
        disabled={disabled || submitting}
        placeholder={t('inputs.enterValue')}
        type={input?.type === 'PromptString' && input.password ? 'password' : 'text'}
        onPressEnter={handleSubmit}
      />
      <ConfigSaveStatus state={submission.state} />
      <Space>
        <Button type="primary" onClick={handleSubmit} loading={submitting} disabled={!canSubmit}>{t('common.save')}</Button>
        <Button disabled={submitting} onClick={cancel}>{t('common.cancel')}</Button>
      </Space>
    </div>
  );
}
