import { useEffect } from 'react';
import { Table, Tag, Button, Collapse, Form, Typography, Space } from 'antd';
import { Input } from '@/components/common/Input';
import { ReloadOutlined, UndoOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-shell';
import { useSettingsStore, type RuntimeInfo } from '@/stores/settingsStore';

const { Text } = Typography;
const { TextArea } = Input;

const INSTALL_URLS: Record<string, string> = {
  'Node.js': 'https://nodejs.org/',
  'Python': 'https://python.org/',
  'uv': 'https://github.com/astral-sh/uv',
  'pnpm': 'https://pnpm.io/',
};

export function RuntimeSettings() {
  const { t } = useTranslation();
  const { runtimes, settings, updateSettings, fetchRuntimes, detectedPath, fetchDetectedPath } = useSettingsStore();

  useEffect(() => {
    fetchRuntimes();
    fetchDetectedPath();
  }, []);

  const columns = [
    { title: t('settings.runtimeName'), dataIndex: 'name', key: 'name' },
    {
      title: t('settings.runtimePath'),
      dataIndex: 'path',
      key: 'path',
      render: (path: string | undefined) =>
        path || <Text type="secondary">{t('settings.notDetected')}</Text>,
    },
    { title: t('settings.runtimeVersion'), dataIndex: 'version', key: 'version' },
    {
      title: t('settings.runtimeStatus'),
      dataIndex: 'available',
      key: 'status',
      render: (available: boolean) =>
        available ? (
          <Tag color="success">{t('settings.installed')}</Tag>
        ) : (
          <Tag color="error">{t('settings.notInstalled')}</Tag>
        ),
    },
    {
      title: '',
      key: 'action',
      render: (_: unknown, record: RuntimeInfo) =>
        !record.available && INSTALL_URLS[record.name] ? (
          <Button type="link" onClick={() => open(INSTALL_URLS[record.name])}>
            {t('settings.installGuide')}
          </Button>
        ) : null,
    },
  ];

  const handleCustomPathChange = (value: string) => {
    if (!settings) return;
    updateSettings({ ...settings, custom_path: value || null });
  };

  const handleResetPath = () => {
    if (!settings) return;
    updateSettings({ ...settings, custom_path: null });
  };

  const handleSkillsRootDirChange = (value: string) => {
    if (!settings) return;
    updateSettings({ ...settings, skills_root_dir: value });
  };

  const handlePathChange = (key: string, value: string) => {
    if (!settings) return;
    const paths = { ...settings.custom_runtime_paths, [key]: value || undefined };
    updateSettings({ ...settings, custom_runtime_paths: paths });
  };

  return (
    <div>
      <Form layout="vertical" style={{ marginBottom: 24 }}>
        <Form.Item
          label={t('settings.pathConfig')}
          help={t('settings.pathDescription')}
        >
          <Space direction="vertical" style={{ width: '100%' }}>
            <TextArea
              value={settings?.custom_path || ''}
              onChange={(e) => handleCustomPathChange(e.target.value)}
              placeholder={detectedPath || t('settings.pathPlaceholder')}
              autoSize={{ minRows: 2, maxRows: 6 }}
              style={{ fontFamily: 'monospace', fontSize: 12 }}
            />
            {settings?.custom_path && (
              <Button
                icon={<UndoOutlined />}
                size="small"
                onClick={handleResetPath}
              >
                {t('settings.resetPath')}
              </Button>
            )}
          </Space>
        </Form.Item>

        <Form.Item
          label={t('settings.skillsRootDir')}
          help={t('settings.skillsRootDirDescription')}
        >
          <Input
            value={settings?.skills_root_dir || '~/.a2c/skills'}
            onChange={(e) => handleSkillsRootDirChange(e.target.value)}
            placeholder="~/.a2c/skills"
          />
        </Form.Item>
      </Form>

      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 16 }}>
        <span />
        <Button icon={<ReloadOutlined />} onClick={fetchRuntimes}>
          {t('common.refresh')}
        </Button>
      </div>

      <Table
        dataSource={runtimes}
        columns={columns}
        rowKey="name"
        pagination={false}
        size="small"
      />

      <Collapse ghost style={{ marginTop: 16 }}>
        <Collapse.Panel header={t('settings.customPaths')} key="paths">
          <Form layout="vertical">
            <Form.Item label="Node.js">
              <Input
                value={settings?.custom_runtime_paths?.node || ''}
                onChange={(e) => handlePathChange('node', e.target.value)}
                placeholder="/usr/local/bin/node"
              />
            </Form.Item>
            <Form.Item label="Python">
              <Input
                value={settings?.custom_runtime_paths?.python || ''}
                onChange={(e) => handlePathChange('python', e.target.value)}
                placeholder="/usr/bin/python3"
              />
            </Form.Item>
            <Form.Item label="uv">
              <Input
                value={settings?.custom_runtime_paths?.uv || ''}
                onChange={(e) => handlePathChange('uv', e.target.value)}
                placeholder="/usr/local/bin/uv"
              />
            </Form.Item>
            <Form.Item label="pnpm">
              <Input
                value={settings?.custom_runtime_paths?.pnpm || ''}
                onChange={(e) => handlePathChange('pnpm', e.target.value)}
                placeholder="/usr/local/bin/pnpm"
              />
            </Form.Item>
          </Form>
        </Collapse.Panel>
      </Collapse>
    </div>
  );
}
