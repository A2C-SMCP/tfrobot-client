import { Form, Input, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import formStyles from '@/components/common/GroupedForm.module.css';
import { useNavigationForm } from '@/components/Navigation/navigationMemoryState';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { ConfigSaveStatus } from './ConfigSaveStatus';
import { useConfigCommit } from './useConfigCommit';

interface Values { name: string; description: string }
export function GeneralSettings({ instance }: { instance: ComputerInstance }) {
  const { t } = useTranslation();
  const [form] = Form.useForm<Values>();
  const patch = useComputerStore((state) => state.patchGeneral);
  const draft = useNavigationForm('settings.general', form, {
    name: instance.name, description: instance.description ?? '',
  });
  const name = useConfigCommit(`${instance.id}:name`, (value: string) => patch(instance.id, { field: 'name', value }));
  const description = useConfigCommit(`${instance.id}:description`, (value: string) => patch(instance.id, { field: 'description', value }));
  const commit = async (field: keyof Values) => {
    const control = field === 'name' ? name : description;
    if (control.state !== 'dirty' && control.state !== 'error' && !draft.dirtyFields.includes(field)) return;
    const current = control.capture();
    const value = form.getFieldValue(field);
    try {
      await form.validateFields([field]);
      if (!current()) return;
      const acknowledge = () => draft.clearFields([field]);
      if (field === 'name') await name.commit(value, acknowledge);
      else await description.commit(value, acknowledge);
    } catch { /* Ant Form retains the invalid edit and displays its validation message. */ }
  };
  return <>
    <Typography.Paragraph type="secondary">{t('computer.redesign.autoSaveHint')}</Typography.Paragraph>
    <Form className={formStyles.form} form={form} layout="vertical" name={`computer-general-${instance.id}`}
    onValuesChange={(changed) => {
      draft.onValuesChange(changed);
      if ('name' in changed) name.dirty();
      if ('description' in changed) description.dirty();
    }}>
    <Form.Item name="name" label={t('computer.form.name')} extra={<ConfigSaveStatus state={name.state === 'saved' && draft.dirtyFields.includes('name') ? 'dirty' : name.state} onRetry={() => commit('name')} />} rules={[{ validator: (_, value) => value?.trim() ? Promise.resolve() : Promise.reject(new Error(t('computer.form.nameRequired'))) }]}>
      <Input autoComplete="off" onBlur={() => void commit('name')} onPressEnter={(event) => { event.preventDefault(); void commit('name'); }} />
    </Form.Item>
    <Form.Item name="description" label={t('computer.form.description')} extra={<ConfigSaveStatus state={description.state === 'saved' && draft.dirtyFields.includes('description') ? 'dirty' : description.state} onRetry={() => commit('description')} />}>
      <Input.TextArea rows={4} onBlur={() => void commit('description')} />
    </Form.Item>

  </Form></>;
}
