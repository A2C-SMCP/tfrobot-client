import { useEffect, useState } from 'react';
import {
  App,
  Button,
  Descriptions,
  Form,
  Input,
  Modal,
  Space,
  Typography,
} from 'antd';
import {
  AppstoreOutlined,
  FolderOpenOutlined,
  ReadOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { formatInvokeError, useSkillStore } from '@/stores/skillStore';

const { Text } = Typography;

interface SkillsSettingsProps {
  instance: ComputerInstance;
  onNavigate?: (key: string) => void;
}

export function SkillsSettings({ instance, onNavigate }: SkillsSettingsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [form] = Form.useForm<{ localSkillsRoot?: string }>();
  const [pendingSkillHomeRoot, setPendingSkillHomeRoot] = useState<string | null | undefined>(
    undefined,
  );
  const loading = useComputerStore((state) => state.loading);
  const updateSkillHome = useComputerStore((state) => state.updateSkillHome);
  const openConfiguredLocalSkillsRoot = useSkillStore(
    (state) => state.openConfiguredLocalSkillsRoot,
  );

  useEffect(() => {
    form.setFieldsValue({ localSkillsRoot: instance.localSkillsRoot ?? '' });
  }, [form, instance.id, instance.localSkillsRoot]);

  const handleChooseSkillHome = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({ directory: true, multiple: false });
      if (path) {
        form.setFieldsValue({ localSkillsRoot: path as string });
      }
    } catch (error) {
      message.error(String(error));
    }
  };

  const handleOpenSkillHome = async () => {
    try {
      await openConfiguredLocalSkillsRoot(instance.id);
    } catch (error) {
      message.error(formatInvokeError(error));
    }
  };

  const applySkillHomeChange = async () => {
    if (pendingSkillHomeRoot === undefined) return;
    try {
      await updateSkillHome(instance.id, pendingSkillHomeRoot);
      setPendingSkillHomeRoot(undefined);
      message.success(t('common.saved'));
    } catch (error) {
      message.error(String(error));
    }
  };

  const handleSaveSkillHome = async () => {
    const values = await form.validateFields();
    setPendingSkillHomeRoot(values.localSkillsRoot?.trim() || null);
  };

  return (
    <>
      <Space direction="vertical" size={20} style={{ width: '100%' }}>
        <Descriptions column={1} size="small">
          <Descriptions.Item label={t('computer.settings.sections.skills.savedSkillHome')}>
            <Text code copyable>
              {instance.localSkillsRoot ?? t('computer.runtime.defaultSkillHome')}
            </Text>
          </Descriptions.Item>
        </Descriptions>

        <Form form={form} layout="vertical">
          <Form.Item label={t('computer.runtime.localSkillsRoot')}>
            <Space.Compact style={{ width: '100%' }}>
              <Form.Item name="localSkillsRoot" noStyle>
                <Input
                  aria-label={t('computer.runtime.localSkillsRoot')}
                  allowClear
                  placeholder={t('computer.runtime.defaultSkillHome')}
                />
              </Form.Item>
              <Button
                icon={<FolderOpenOutlined />}
                onClick={handleChooseSkillHome}
                aria-label={t('computer.runtime.chooseSkillHome')}
              />
            </Space.Compact>
          </Form.Item>
          <Space wrap>
            <Button type="primary" loading={loading} onClick={handleSaveSkillHome}>
              {t('common.save')}
            </Button>
            <Button
              loading={loading}
              onClick={() => {
                form.setFieldsValue({ localSkillsRoot: '' });
                setPendingSkillHomeRoot(null);
              }}
            >
              {t('computer.runtime.resetSkillHome')}
            </Button>
            <Button
              icon={<FolderOpenOutlined />}
              aria-label={t('computer.settings.sections.skills.openDirectory')}
              onClick={() => void handleOpenSkillHome()}
            >
              {t('computer.settings.sections.skills.openDirectory')}
            </Button>
          </Space>
        </Form>
        <Text type="secondary">{t('computer.runtime.skillHomeDescription')}</Text>

        <Space wrap>
          <Button
            icon={<ReadOutlined />}
            aria-label={t('computer.settings.sections.skills.openActiveSkills')}
            onClick={() => onNavigate?.('computer-detail:skills')}
          >
            {t('computer.settings.sections.skills.openActiveSkills')}
          </Button>
          <Button
            icon={<AppstoreOutlined />}
            aria-label={t('computer.settings.sections.skills.openPlugins')}
            onClick={() => onNavigate?.('computer-settings:plugins')}
          >
            {t('computer.settings.sections.skills.openPlugins')}
          </Button>
        </Space>
      </Space>

      <Modal
        title={t('computer.runtime.skillHomeConfirmTitle')}
        open={pendingSkillHomeRoot !== undefined}
        okText={t('computer.runtime.skillHomeConfirmOk')}
        cancelText={t('common.cancel')}
        confirmLoading={loading}
        onOk={() => void applySkillHomeChange()}
        onCancel={() => setPendingSkillHomeRoot(undefined)}
      >
        <Space direction="vertical" size={12}>
          <Text>{t('computer.runtime.skillHomeConfirmDescription')}</Text>
          <Descriptions column={1} size="small">
            <Descriptions.Item label={t('computer.settings.sections.skills.savedSkillHome')}>
              <Text code>
                {instance.localSkillsRoot ?? t('computer.runtime.defaultSkillHome')}
              </Text>
            </Descriptions.Item>
            <Descriptions.Item label={t('computer.runtime.nextSkillHome')}>
              <Text code>
                {pendingSkillHomeRoot || t('computer.runtime.defaultSkillHome')}
              </Text>
            </Descriptions.Item>
          </Descriptions>
        </Space>
      </Modal>
    </>
  );
}
