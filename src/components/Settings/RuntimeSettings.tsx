import { useEffect, useMemo, useState } from 'react';
import { Table, Tag, Button, Collapse, Form, Typography, Space } from 'antd';
import { Input } from '@/components/common/Input';
import { UndoOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-shell';
import { useSettingsStore, type AppSettings, type CustomRuntimePaths, type RuntimeInfo } from '@/stores/settingsStore';

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
  const [customPathDraft, setCustomPathDraft] = useState('');
  const [skillsRootDraft, setSkillsRootDraft] = useState('');
  const [customRuntimePathsDraft, setCustomRuntimePathsDraft] = useState<CustomRuntimePaths>({});

  useEffect(() => {
    fetchRuntimes();
    fetchDetectedPath();
  }, []);

  useEffect(() => {
    setCustomPathDraft(settings?.custom_path || '');
    setSkillsRootDraft(settings?.skills_root_dir || '~/.a2c/skills');
    setCustomRuntimePathsDraft(settings?.custom_runtime_paths || {});
  }, [
    settings?.custom_path,
    settings?.skills_root_dir,
    settings?.custom_runtime_paths,
  ]);

  const hasRuntimeSettingsChanges = useMemo(() => {
    if (!settings) return false;
    return (
      (customPathDraft || null) !== (settings.custom_path || null) ||
      skillsRootDraft !== settings.skills_root_dir ||
      normalizeRuntimePaths(customRuntimePathsDraft) !== normalizeRuntimePaths(settings.custom_runtime_paths)
    );
  }, [customPathDraft, customRuntimePathsDraft, settings, skillsRootDraft]);

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
    setCustomPathDraft(value);
  };

  const handleResetPath = () => {
    setCustomPathDraft('');
  };

  const resetDraftsFromSettings = () => {
    setCustomPathDraft(settings?.custom_path || '');
    setSkillsRootDraft(settings?.skills_root_dir || '~/.a2c/skills');
    setCustomRuntimePathsDraft(settings?.custom_runtime_paths || {});
  };

  const handleSaveRuntimeSettings = () => {
    if (!settings) return;
    const updated: AppSettings = {
      ...settings,
      custom_path: customPathDraft || null,
      skills_root_dir: skillsRootDraft,
      custom_runtime_paths: customRuntimePathsDraft,
    };
    if (!hasRuntimeSettingsChanges) {
      return;
    }
    updateSettings(updated);
  };

  const handlePathChange = (key: string, value: string) => {
    setCustomRuntimePathsDraft((paths) => ({ ...paths, [key]: value || undefined }));
  };

  return (
    <div>
      <Form layout="vertical" style={{ marginBottom: 24 }}>
        <Collapse
          defaultActiveKey={['computer', 'paths']}
          items={[
            {
              key: 'computer',
              label: t('settings.computerConfig'),
              children: (
                <>
                  <Form.Item
                    label={t('settings.skillsRootDir')}
                    help={t('settings.skillsRootDirDescription')}
                  >
                    <Input
                      value={skillsRootDraft}
                      onChange={(e) => setSkillsRootDraft(e.target.value)}
                      placeholder="~/.a2c/skills"
                    />
                  </Form.Item>
                </>
              ),
            },
            {
              key: 'paths',
              label: t('settings.pathConfig'),
              children: (
                <>
                  <Form.Item
                    label="PATH"
                    help={t('settings.pathDescription')}
                  >
                    <Space direction="vertical" style={{ width: '100%' }}>
                      <TextArea
                        value={customPathDraft}
                        onChange={(e) => handleCustomPathChange(e.target.value)}
                        placeholder={detectedPath || t('settings.pathPlaceholder')}
                        autoSize={{ minRows: 2, maxRows: 6 }}
                        style={{ fontFamily: 'monospace', fontSize: 12 }}
                      />
                      {customPathDraft && (
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

                  <Form.Item label="Node.js">
                    <Input
                      value={customRuntimePathsDraft.node || ''}
                      onChange={(e) => handlePathChange('node', e.target.value)}
                      placeholder="/usr/local/bin/node"
                    />
                  </Form.Item>
                  <Form.Item label="Python">
                    <Input
                      value={customRuntimePathsDraft.python || ''}
                      onChange={(e) => handlePathChange('python', e.target.value)}
                      placeholder="/usr/bin/python3"
                    />
                  </Form.Item>
                  <Form.Item label="uv">
                    <Input
                      value={customRuntimePathsDraft.uv || ''}
                      onChange={(e) => handlePathChange('uv', e.target.value)}
                      placeholder="/usr/local/bin/uv"
                    />
                  </Form.Item>
                  <Form.Item label="pnpm">
                    <Input
                      value={customRuntimePathsDraft.pnpm || ''}
                      onChange={(e) => handlePathChange('pnpm', e.target.value)}
                      placeholder="/usr/local/bin/pnpm"
                    />
                  </Form.Item>
                </>
              ),
            },
          ]}
        />
      </Form>

      <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 16 }}>
        <Space>
          <Button
            disabled={!hasRuntimeSettingsChanges}
            onClick={resetDraftsFromSettings}
          >
            {t('common.cancel')}
          </Button>
          <Button
            type={hasRuntimeSettingsChanges ? 'primary' : 'default'}
            disabled={!hasRuntimeSettingsChanges}
            onClick={handleSaveRuntimeSettings}
          >
            {t('common.save')}
          </Button>
        </Space>
      </div>

      <Table
        dataSource={runtimes}
        columns={columns}
        rowKey="name"
        pagination={false}
        size="small"
      />
    </div>
  );
}

function normalizeRuntimePaths(paths: CustomRuntimePaths | undefined) {
  return JSON.stringify({
    node: paths?.node || '',
    python: paths?.python || '',
    uv: paths?.uv || '',
    pnpm: paths?.pnpm || '',
  });
}
