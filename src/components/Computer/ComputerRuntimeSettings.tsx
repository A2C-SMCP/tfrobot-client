import { useEffect } from 'react';
import { App, Button, Card, Descriptions, Form, Input, Space, Switch, Typography } from 'antd';
import { FolderOpenOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';

const { Text } = Typography;

interface ComputerRuntimeSettingsProps {
  instance: ComputerInstance;
}

export function ComputerRuntimeSettings({ instance }: ComputerRuntimeSettingsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [form] = Form.useForm<{ localSkillsRoot?: string }>();
  const { loading, updateConnectionPolicy, updateSkillHome } = useComputerStore();
  const target = instance.connectionPolicy.target;
  const autoConnect = instance.connectionPolicy.auto_connect;

  useEffect(() => {
    form.setFieldsValue({ localSkillsRoot: instance.localSkillsRoot ?? '' });
  }, [form, instance.id, instance.localSkillsRoot]);

  const handleAutoConnectChange = async (checked: boolean) => {
    try {
      await updateConnectionPolicy(instance.id, {
        target: target ?? null,
        auto_connect: checked,
      });
      message.success(t('common.saved'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleChooseSkillHome = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({ directory: true, multiple: false });
      if (path) {
        form.setFieldsValue({ localSkillsRoot: path as string });
      }
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleSaveSkillHome = async () => {
    const values = await form.validateFields();
    try {
      await updateSkillHome(instance.id, values.localSkillsRoot?.trim() || null);
      message.success(t('common.saved'));
    } catch (e) {
      message.error(String(e));
    }
  };

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small" title={t('computer.runtime.policyTitle')}>
        <Descriptions column={1} size="small">
          <Descriptions.Item label={t('computer.runtime.instance')}>
            <Space direction="vertical" size={0}>
              <Text strong>{instance.name}</Text>
              <Text type="secondary" copyable>{instance.id}</Text>
            </Space>
          </Descriptions.Item>
          <Descriptions.Item label={t('computer.runtime.connectionTarget')}>
            {target ? (
              <Text code>
                {target.type}:{target.id}
              </Text>
            ) : (
              <Text type="secondary">{t('computer.runtime.noConnectionTarget')}</Text>
            )}
          </Descriptions.Item>
          <Descriptions.Item label={t('connection.form.autoConnect')}>
            <Space>
              <Switch
                checked={autoConnect}
                disabled={!target}
                loading={loading}
                onChange={handleAutoConnectChange}
              />
              <Text type="secondary">
                {target
                  ? t('computer.runtime.autoConnectDescription')
                  : t('computer.runtime.autoConnectRequiresTarget')}
              </Text>
            </Space>
          </Descriptions.Item>
        </Descriptions>
      </Card>

      <Card size="small" title={t('computer.runtime.lifecycleTitle')}>
        <Space direction="vertical" size={8}>
          <Text>{t('computer.runtime.startBehavior')}</Text>
          <Text type="secondary">{t('computer.runtime.globalRuntimeHint')}</Text>
          <Button onClick={() => useComputerStore.getState().startInstance(instance.id)} loading={loading}>
            {t('computer.start')}
          </Button>
        </Space>
      </Card>

      <Card size="small" title={t('computer.runtime.skillHomeTitle')}>
        <Space direction="vertical" size={12} style={{ width: '100%' }}>
          <Descriptions column={1} size="small">
            <Descriptions.Item label={t('computer.runtime.effectiveSkillHome')}>
              <Text code copyable>{instance.effectiveSkillHome ?? t('computer.runtime.defaultSkillHome')}</Text>
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
                <TooltipButton
                  label={t('computer.runtime.chooseSkillHome')}
                  onClick={handleChooseSkillHome}
                />
              </Space.Compact>
            </Form.Item>
            <Space wrap>
              <Button type="primary" loading={loading} onClick={handleSaveSkillHome}>
                {t('common.save')}
              </Button>
              <Button
                loading={loading}
                onClick={async () => {
                  form.setFieldsValue({ localSkillsRoot: '' });
                  try {
                    await updateSkillHome(instance.id, null);
                    message.success(t('common.saved'));
                  } catch (e) {
                    message.error(String(e));
                  }
                }}
              >
                {t('computer.runtime.resetSkillHome')}
              </Button>
            </Space>
          </Form>
          <Text type="secondary">{t('computer.runtime.skillHomeDescription')}</Text>
        </Space>
      </Card>
    </Space>
  );
}

function TooltipButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <Button
      icon={<FolderOpenOutlined />}
      onClick={onClick}
      aria-label={label}
    />
  );
}
