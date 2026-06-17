import { useEffect, useState } from 'react';
import { Alert, App, Button, Empty, Modal, Space, Spin, Table, Typography } from 'antd';
import { CloseOutlined, FileTextOutlined, FolderOpenOutlined, SyncOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useSkillsStore, type SkillInfo } from '@/stores/skillsStore';

const { Paragraph, Text, Title } = Typography;

function isLocalSkill(skill: SkillInfo | null): skill is SkillInfo {
  return skill?.source === 'local';
}

function renderInlineCode(text: string) {
  const parts = text.split(/(`[^`]+`)/g);
  return parts.map((part, index) => {
    if (part.startsWith('`') && part.endsWith('`')) {
      return <Text code key={index}>{part.slice(1, -1)}</Text>;
    }
    return part;
  });
}

function MarkdownPreview({ content }: { content: string }) {
  const lines = content.split(/\r?\n/);
  const blocks = [];
  let codeBlock: string[] | null = null;

  for (const line of lines) {
    if (line.startsWith('```')) {
      if (codeBlock) {
        blocks.push({ type: 'code', content: codeBlock.join('\n') });
        codeBlock = null;
      } else {
        codeBlock = [];
      }
      continue;
    }

    if (codeBlock) {
      codeBlock.push(line);
      continue;
    }

    blocks.push({ type: 'line', content: line });
  }

  if (codeBlock) {
    blocks.push({ type: 'code', content: codeBlock.join('\n') });
  }

  return (
    <div>
      {blocks.map((block, index) => {
        if (block.type === 'code') {
          return (
            <pre
              key={index}
              style={{
                background: '#f5f7fa',
                border: '1px solid #e5e7eb',
                borderRadius: 4,
                margin: '12px 0',
                overflow: 'auto',
                padding: '12px 16px',
                whiteSpace: 'pre-wrap',
              }}
            >
              <code>{block.content}</code>
            </pre>
          );
        }

        const line = block.content;
        if (!line.trim()) {
          return <div key={index} style={{ height: 10 }} />;
        }
        if (line.startsWith('### ')) {
          return <Title key={index} level={5}>{renderInlineCode(line.slice(4))}</Title>;
        }
        if (line.startsWith('## ')) {
          return <Title key={index} level={4}>{renderInlineCode(line.slice(3))}</Title>;
        }
        if (line.startsWith('# ')) {
          return <Title key={index} level={3}>{renderInlineCode(line.slice(2))}</Title>;
        }
        if (line.startsWith('- ') || line.startsWith('* ')) {
          return (
            <Paragraph key={index} style={{ marginBottom: 8 }}>
              {'• '}
              {renderInlineCode(line.slice(2))}
            </Paragraph>
          );
        }
        return <Paragraph key={index}>{renderInlineCode(line)}</Paragraph>;
      })}
    </div>
  );
}

export function Skills() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [previewOpen, setPreviewOpen] = useState(false);
  const {
    skills,
    syncSummary,
    loading,
    syncing,
    opening,
    error,
    selectedSkillPath,
    markdownByPath,
    previewLoading,
    previewError,
    fetchSkills,
    syncSkills,
    openSkillsRoot,
    openSkillFolder,
    openSkillMarkdownFile,
    selectSkill,
  } = useSkillsStore();

  useEffect(() => {
    fetchSkills();
  }, [fetchSkills]);

  const selectedSkill = skills.find((skill) => skill.path === selectedSkillPath) ?? null;
  const selectedMarkdown = selectedSkill ? markdownByPath[selectedSkill.path] : undefined;
  const conflicts = syncSummary?.ignored_conflicts ?? [];
  const skipped = syncSummary?.skipped ?? [];

  const handleOpenRoot = async () => {
    try {
      await openSkillsRoot();
    } catch {
      message.error(t('skills.openRootFailed'));
    }
  };

  const handleSyncSkills = async () => {
    try {
      await syncSkills();
    } catch {
      message.error(t('skills.syncFailed'));
    }
  };

  const handleOpenSkillFolder = async (skill: SkillInfo) => {
    try {
      await openSkillFolder(skill);
    } catch {
      message.error(t('skills.openFolderFailed'));
    }
  };

  const handleOpenMarkdownFile = async () => {
    if (!selectedSkill) {
      return;
    }
    try {
      await openSkillMarkdownFile(selectedSkill);
    } catch {
      message.error(t('skills.openFileFailed'));
    }
  };

  const handlePreview = async (skill: SkillInfo) => {
    setPreviewOpen(true);
    await selectSkill(skill);
  };

  const renderPreview = () => {
    if (!selectedSkill) {
      return <Empty description={t('skills.previewSelect')} />;
    }

    if (!selectedSkill.has_skill_md || previewError === 'missing_skill_md') {
      return <Empty description={t('skills.previewMissing')} />;
    }

    if (previewLoading) {
      return (
        <div style={{ padding: '48px 0', textAlign: 'center' }}>
          <Spin />
        </div>
      );
    }

    if (previewError) {
      return (
        <Alert
          message={t('skills.previewFailed')}
          description={previewError}
          type="error"
          showIcon
        />
      );
    }

    if (selectedMarkdown !== undefined && !selectedMarkdown.trim()) {
      return <Empty description={t('skills.previewEmpty')} />;
    }

    if (selectedMarkdown === undefined) {
      return <Empty description={t('skills.previewSelect')} />;
    }

    return <MarkdownPreview content={selectedMarkdown} />;
  };

  const columns = [
    {
      title: t('skills.columns.name'),
      dataIndex: 'name',
      key: 'name',
      width: '24%',
      render: (name: string) => <Text>{name}</Text>,
    },
    {
      title: t('skills.columns.description'),
      dataIndex: 'description',
      key: 'description',
      render: (description: string | null) => (
        <Text type={description ? undefined : 'secondary'}>
          {description || t('skills.noDescription')}
        </Text>
      ),
    },
    {
      title: t('skills.columns.source'),
      dataIndex: 'source',
      key: 'source',
      width: 140,
      render: (source: string) => <Text type="secondary">{source}</Text>,
    },
    {
      title: t('skills.columns.actions'),
      key: 'actions',
      width: 180,
      align: 'right' as const,
      render: (_: unknown, skill: SkillInfo) => (
        <Space size={8}>
          <Button
            size="small"
            icon={<FileTextOutlined />}
            onClick={() => void handlePreview(skill)}
          >
            {t('skills.previewAction')}
          </Button>
          {isLocalSkill(skill) && (
            <Button
              size="small"
              icon={<FolderOpenOutlined />}
              onClick={() => void handleOpenSkillFolder(skill)}
            >
              {t('skills.folderAction')}
            </Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div
        style={{
          alignItems: 'flex-start',
          display: 'flex',
          justifyContent: 'space-between',
          gap: 16,
          marginBottom: 16,
        }}
      >
        <div>
          <Title level={3} style={{ margin: 0 }}>
            {t('skills.title')}
          </Title>
          <Text type="secondary">{t('skills.subtitle')}</Text>
        </div>
        <Space>
          <Button
            icon={<SyncOutlined />}
            onClick={() => void handleSyncSkills()}
            loading={syncing}
          >
            {t('skills.sync')}
          </Button>
          <Button
            icon={<FolderOpenOutlined />}
            onClick={handleOpenRoot}
            loading={opening}
          >
            {t('skills.openRootShort')}
          </Button>
        </Space>
      </div>

      {error && (
        <Alert
          message={t('common.error')}
          description={error}
          type="error"
          showIcon
          closable
          style={{ marginBottom: 16 }}
        />
      )}

      {syncSummary && (
        <Alert
          message={t('skills.summary.title')}
          description={
            <Space size={16} wrap>
              <Text>{t('skills.summary.localSynced', { count: syncSummary.local_synced })}</Text>
              <Text>{t('skills.summary.mcpSynced', { count: syncSummary.mcp_synced })}</Text>
              <Text>{t('skills.summary.skipped', { count: skipped.length })}</Text>
              <Text>{t('skills.summary.conflicts', { count: conflicts.length })}</Text>
            </Space>
          }
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      {conflicts.length > 0 && (
        <Alert
          message={t('skills.conflicts.title')}
          description={
            <Space direction="vertical" size={4}>
              <Text>{t('skills.conflicts.description')}</Text>
              {conflicts.map((conflict) => (
                <Text key={`${conflict.ignored_name}:${conflict.reason}`}>
                  {t('skills.conflicts.item', {
                    skill: conflict.skill_name,
                    kept: conflict.kept_name,
                    ignored: conflict.ignored_name,
                    reason: conflict.reason,
                  })}
                </Text>
              ))}
            </Space>
          }
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      {skipped.length > 0 && (
        <Alert
          message={t('skills.skipped.title')}
          description={
            <Space direction="vertical" size={4}>
              <Text>{t('skills.skipped.description')}</Text>
              {skipped.map((entry) => (
                <Text key={`${entry.source}:${entry.skill_name}:${entry.reason}`}>
                  {t('skills.skipped.item', {
                    skill: entry.skill_name,
                    source: entry.source,
                    reason: entry.reason,
                  })}
                </Text>
              ))}
            </Space>
          }
          type="warning"
          showIcon
          style={{ marginBottom: 16 }}
        />
      )}

      <Table
        columns={columns}
        dataSource={skills}
        rowKey="path"
        loading={loading}
        pagination={false}
        locale={{ emptyText: <Empty description={t('skills.empty')} /> }}
      />

      <Modal
        title={selectedSkill ? `${selectedSkill.name} / SKILL.md` : t('skills.previewTitle')}
        open={previewOpen}
        width={900}
        closeIcon={<CloseOutlined />}
        onCancel={() => setPreviewOpen(false)}
        footer={[
          <Button key="close" onClick={() => setPreviewOpen(false)}>
            {t('common.close')}
          </Button>,
          ...(isLocalSkill(selectedSkill)
            ? [
                <Button
                  key="open"
                  type="primary"
                  disabled={!selectedSkill.has_skill_md}
                  onClick={() => void handleOpenMarkdownFile()}
                >
                  {t('skills.openFile')}
                </Button>,
              ]
            : []),
        ]}
      >
        <div style={{ maxHeight: '62vh', overflow: 'auto', padding: '8px 0' }}>
          {renderPreview()}
        </div>
      </Modal>
    </div>
  );
}
