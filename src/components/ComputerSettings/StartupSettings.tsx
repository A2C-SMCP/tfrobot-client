import { Card, Form, InputNumber, Space, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useNavigationForm } from '@/components/Navigation/navigationMemoryState';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { ConfigSaveStatus } from './ConfigSaveStatus';
import { useConfigCommit } from './useConfigCommit';

export function StartupSettings({ instance }: { instance: ComputerInstance }) {
  const { t } = useTranslation();
  const [form] = Form.useForm<{ concurrency: number }>();
  const patch = useComputerStore((state) => state.patchGeneral);
  const initialValues = { concurrency: instance.mcpStartConcurrency ?? 5 };
  const draft = useNavigationForm('settings.startup', form, initialValues);
  const submission = useConfigCommit(`${instance.id}:concurrency`, (value: number) => patch(instance.id, { field: 'mcpStartConcurrency', value }));
  const save = async () => {
    if (!draft.dirtyFields.length && submission.state !== 'error') return;
    const current = submission.capture();
    try {
      const { concurrency } = await form.validateFields();
      if (current()) await submission.commit(concurrency, () => draft.clearDraft());
    } catch { /* The form explains invalid values without submitting them. */ }
  };
  return <Card size="small" title={t('computer.redesign.startupTitle')}>
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Typography.Text type="secondary">{t('computer.redesign.autoSaveHint')}</Typography.Text>
      <Form form={form} initialValues={initialValues} name={`computer-startup-${instance.id}`} layout="vertical" onValuesChange={(changed) => { draft.onValuesChange(changed); submission.dirty(); }}>
        <Form.Item name="concurrency" label={t('computer.form.mcpStartConcurrency')}
          rules={[{ required: true, type: 'number', min: 1, max: 64 }]}
          extra={<Space direction="vertical">{t('computer.form.mcpStartConcurrencyHint')}<ConfigSaveStatus state={submission.state === 'saved' && draft.dirtyFields.length ? 'dirty' : submission.state} onRetry={() => void save()} /></Space>}>
          <InputNumber min={1} max={64} precision={0} onBlur={() => void save()} onPressEnter={(event) => { event.preventDefault(); void save(); }} />
        </Form.Item>
      </Form>
    </Space>
  </Card>;
}
