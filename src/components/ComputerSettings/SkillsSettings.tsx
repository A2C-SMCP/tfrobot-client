import { ConfigSaveStatus } from './ConfigSaveStatus';
import { useConfigCommit } from './useConfigCommit';
import { useNavigationForm } from '@/components/Navigation/navigationMemoryState';
import { usePageAction, usePageActive } from '@/components/Navigation/pageActivityState';
import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import { useEffect, useRef, useState } from 'react';
import { Alert, App, Button, Card, Form, Input, Radio, Space, Tag, Typography } from 'antd';
import { FolderOpenOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { formatInvokeError, useSkillStore } from '@/stores/skillStore';
import styles from './ComputerSettings.module.css';

const { Text, Paragraph } = Typography;
interface StorageValues {
  mode: 'default' | 'custom';
  localSkillsRoot: string;
}
interface SkillsSettingsProps {
  instance: ComputerInstance;
  onNavigate?: (key: string) => void;
}

/** Advanced storage settings. The runtime workbench owns the usable skill list. */
export function SkillsSettings({ instance }: SkillsSettingsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [form] = Form.useForm<StorageValues>();
  const [editing, setEditing] = useState(false);
  const editSession = useRef(0);
  const active = usePageActive();
  const action = usePageAction(instance.id);
  const updateSkillHome = useComputerStore((state) => state.updateSkillHome);
  const openConfiguredLocalSkillsRoot = useSkillStore((state) => state.openConfiguredLocalSkillsRoot);
  const submission = useConfigCommit(`settings.skillHome.${instance.id}`, (root: string | null) => updateSkillHome(instance.id, root));
  const saving = submission.state === 'saving';
  const initial: StorageValues = { mode: instance.localSkillsRoot ? 'custom' : 'default', localSkillsRoot: instance.localSkillsRoot ?? '' };
  const draft = useNavigationForm('settings.skillHome', form, initial);
  const mode = Form.useWatch('mode', form) ?? initial.mode;
  const defaultSkillHome = instance.defaultSkillHome ?? (instance.localSkillsRoot ? '' : instance.effectiveSkillHome) ?? '';
  const configuredSkillHome = instance.configuredSkillHome ?? instance.localSkillsRoot ?? instance.effectiveSkillHome ?? '';

  useEffect(() => {
    if (!active) { editSession.current += 1; setEditing(false); }
  }, [active]);

  const cancel = () => {
    if (saving) return;
    editSession.current += 1;
    form.setFieldsValue(initial);
    draft.clearDraft();
    submission.reset();
    setEditing(false);
  };
  const chooseFolder = async () => {
    const current = action();
    const session = editSession.current;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      if (!current() || editSession.current !== session) return;
      const path = await open({ directory: true, multiple: false });
      if (path && current() && editSession.current === session) {
        draft.setValues({ mode: 'custom', localSkillsRoot: path as string });
        submission.dirty();
      }
    } catch (error) {
      if (current() && editSession.current === session) message.error(formatInvokeError(error));
    }
  };
  const save = async () => {
    const current = action();
    const session = editSession.current;
    let values: StorageValues;
    try { values = await form.validateFields(); }
    catch { return; }
    if (!current() || editSession.current !== session) return;
    const root = values.mode === 'default' ? null : values.localSkillsRoot.trim();
    const acknowledged = await submission.commit(root, () => draft.clearDraft());
    if (acknowledged && current() && editSession.current === session) setEditing(false);
  };
  const openDirectory = async () => {
    const current = action();
    try { await openConfiguredLocalSkillsRoot(instance.id); }
    catch (error) { if (current()) message.error(formatInvokeError(error)); }
  };

  return <>
    <Card size="small" title={t('computer.storage.title')} role="region" aria-label={t('computer.storage.title')}>
      <Space direction="vertical" size={16} style={{ width: '100%', minWidth: 0 }}>
        <Paragraph type="secondary" style={{ margin: 0 }}>{t('computer.storage.description')}</Paragraph>
        <Space><Text strong>{t('computer.storage.current')}</Text><Tag>{t(`computer.storage.${initial.mode}`)}</Tag></Space>
        <Text code copyable className={styles.storagePath} title={configuredSkillHome}>{configuredSkillHome}</Text>
        <Space wrap>
          <Button aria-label={t('computer.storage.openSaved')} icon={<FolderOpenOutlined />} onClick={() => void openDirectory()}>{t('computer.storage.openSaved')}</Button>
          <Button disabled={saving} onClick={() => { editSession.current += 1; setEditing(true); }}>{t('computer.storage.change')}</Button>
          {submission.state !== 'saved' && <ConfigSaveStatus state={submission.state} />}
        </Space>
      </Space>
    </Card>
    <Modal forceRender title={t('computer.storage.change')} open={editing} okText={t('common.save')} cancelText={t('common.cancel')}
      confirmLoading={saving} okButtonProps={{ disabled: submission.state === 'saved' && draft.dirtyFields.length === 0 }}
      cancelButtonProps={{ disabled: saving }} closable={!saving} maskClosable={!saving}
      onOk={() => void save()} onCancel={cancel}>
      <Space direction="vertical" size={16} style={{ width: '100%' }}>
        <Text>{t('computer.storage.changeDescription')}</Text>
        <Alert type="info" showIcon message={t('computer.storage.noMigration')} />
        <Form name={`computer-storage-${instance.id}`} form={form} layout="vertical" disabled={saving}
          onValuesChange={(changed) => { draft.onValuesChange(changed); submission.dirty(); }}>
          <Form.Item name="mode" label={t('computer.storage.location')}>
            <Radio.Group options={[{ value: 'default', label: t('computer.storage.default') }, { value: 'custom', label: t('computer.storage.custom') }]} />
          </Form.Item>
          {mode === 'default' ? <Text code className={styles.storagePath}>{defaultSkillHome || t('computer.storage.defaultResolved')}</Text> :
            <Form.Item label={t('computer.storage.customPath')} required>
              <Space.Compact style={{ width: '100%' }}>
                <Form.Item name="localSkillsRoot" noStyle rules={[{ required: true, whitespace: true, message: t('computer.storage.pathRequired') }]}>
                  <Input aria-label={t('computer.storage.customPath')} />
                </Form.Item>
                <Button icon={<FolderOpenOutlined />} aria-label={t('computer.runtime.chooseSkillHome')} onClick={() => void chooseFolder()} />
              </Space.Compact>
            </Form.Item>}
          <div style={{ marginTop: 16 }}><ConfigSaveStatus state={submission.state === 'saved' && draft.dirtyFields.length ? 'dirty' : submission.state} /></div>
          {submission.state === 'error' && <Alert type="error" showIcon message={formatInvokeError(submission.getError())} />}
        </Form>
      </Space>
    </Modal>
  </>;
}
