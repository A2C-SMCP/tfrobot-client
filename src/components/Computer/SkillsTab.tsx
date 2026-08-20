import { useEffect, useMemo, useState } from 'react';
import { App, Alert, Button, Empty, Input, List, Skeleton, Space, Tag, Typography } from 'antd';
import { FolderOpenOutlined, ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { formatInvokeError, useSkillStore, type SkillRef } from '@/stores/skillStore';
import styles from './SkillsTab.module.css';

const { Text, Title, Paragraph } = Typography;
const EMPTY_SKILLS: SkillRef[] = [];

interface SkillsTabProps {
  instanceId: string;
}

function groupBySource(skills: SkillRef[]) {
  return skills.reduce<Record<string, SkillRef[]>>((groups, skill) => {
    groups[skill.source] = groups[skill.source] ?? [];
    groups[skill.source].push(skill);
    return groups;
  }, {});
}

function renderMarkdown(markdown: string) {
  const blocks = markdown.split(/\n{2,}/);

  return blocks.map((block, index) => {
    const trimmed = block.trim();
    if (!trimmed) return null;
    if (trimmed.startsWith('```')) {
      return (
        <pre key={index} style={{ overflow: 'auto', background: '#f5f5f5', padding: 12, borderRadius: 6 }}>
          {trimmed.replace(/^```[^\n]*\n?/, '').replace(/\n?```$/, '')}
        </pre>
      );
    }
    if (trimmed.startsWith('# ')) {
      return <Title key={index} level={4}>{trimmed.replace(/^# /, '')}</Title>;
    }
    if (trimmed.startsWith('## ')) {
      return <Title key={index} level={5}>{trimmed.replace(/^## /, '')}</Title>;
    }
    if (/^[-*] /m.test(trimmed)) {
      return (
        <ul key={index} style={{ paddingLeft: 20 }}>
          {trimmed.split('\n').map((line) => (
            <li key={line}>{line.replace(/^[-*] /, '')}</li>
          ))}
        </ul>
      );
    }
    return <Paragraph key={index} style={{ whiteSpace: 'pre-wrap' }}>{trimmed}</Paragraph>;
  });
}

export function SkillsTab({ instanceId }: SkillsTabProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [search, setSearch] = useState('');
  const {
    recordsByInstanceId,
    fetchSkills,
    refreshSkills,
    selectSkill,
    openLocalSkillsRoot,
  } = useSkillStore();
  const record = recordsByInstanceId[instanceId];
  const skills = record?.skills ?? EMPTY_SKILLS;
  const selectedSkillName = record?.selectedSkillName ?? null;
  const selectedSkill = record?.selectedSkill ?? null;
  const loadingSkills = record?.loadingSkills ?? false;
  const loadingSkill = record?.loadingSkill ?? false;
  const error = record?.error ?? null;
  const skillError = record?.skillError ?? null;

  useEffect(() => {
    fetchSkills(instanceId);
  }, [fetchSkills, instanceId]);

  const filteredSkills = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return skills;
    return skills.filter((skill) => {
      return [skill.name, skill.description, skill.source]
        .some((value) => value.toLowerCase().includes(query));
    });
  }, [search, skills]);
  const grouped = useMemo(() => groupBySource(filteredSkills), [filteredSkills]);

  const handleRefresh = async () => {
    try {
      await refreshSkills(instanceId);
      message.success(t('skills.messages.refreshed'));
    } catch (e) {
      message.error(formatInvokeError(e));
    }
  };

  const handleOpenRoot = async () => {
    try {
      await openLocalSkillsRoot(instanceId);
    } catch (e) {
      message.error(formatInvokeError(e));
    }
  };

  const renderSkillDetail = () => {
    if (!selectedSkillName) {
      return <Empty description={t('skills.emptySelection')} />;
    }
    if (loadingSkill) {
      return <Skeleton active paragraph={{ rows: 8 }} />;
    }
    if (skillError) {
      return <Alert type="warning" showIcon message={t('skills.resourceUnavailable')} description={skillError} />;
    }
    if (!selectedSkill) {
      return <Alert type="warning" showIcon message={t('skills.resourceUnavailable')} description={t('skills.missingSkillMd')} />;
    }
    if (!selectedSkill.body) {
      return (
        <Alert
          type="warning"
          showIcon
          message={t('skills.emptySkillMd')}
          description={selectedSkill.isText ? t('skills.emptySkillMdDescription') : t('skills.nonTextSkillMd')}
        />
      );
    }

    return renderMarkdown(selectedSkill.body);
  };

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('skills.title')}</Title>
        <Space wrap>
          <Button icon={<ReloadOutlined />} loading={loadingSkills} onClick={handleRefresh}>
            {t('common.refresh')}
          </Button>
          <Button icon={<FolderOpenOutlined />} disabled={loadingSkills} onClick={handleOpenRoot}>
            {t('skills.openLocalRoot')}
          </Button>
        </Space>
      </div>

      {error && <Alert type="error" showIcon message={t('common.error')} description={error} />}

      <div className={styles.layout}>
        <div className={styles.listPane}>
          <Input
            allowClear
            prefix={<SearchOutlined />}
            placeholder={t('skills.searchPlaceholder')}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
          {loadingSkills ? (
            <Skeleton active paragraph={{ rows: 8 }} />
          ) : skills.length === 0 ? (
            <Empty description={t('skills.empty')} />
          ) : filteredSkills.length === 0 ? (
            <Empty description={t('skills.emptySearch')} />
          ) : (
            <Space direction="vertical" size={12} className={styles.list}>
              {Object.entries(grouped).map(([source, sourceSkills]) => (
                <List
                  key={source}
                  size="small"
                  header={<Tag color={source === 'user' ? 'green' : source.startsWith('mcp:') ? 'blue' : 'purple'}>{source}</Tag>}
                  bordered
                  dataSource={sourceSkills}
                  renderItem={(skill) => (
                    <List.Item
                      style={{ background: selectedSkillName === skill.name ? '#f6ffed' : undefined }}
                    >
                      <button
                        type="button"
                        className={styles.skillButton}
                        aria-pressed={selectedSkillName === skill.name}
                        onClick={() => selectSkill(instanceId, skill.name)}
                      >
                        <Space direction="vertical" size={2} style={{ width: '100%', minWidth: 0 }}>
                          <Text strong ellipsis>{skill.name}</Text>
                          <Text type="secondary" ellipsis>{skill.description}</Text>
                        </Space>
                      </button>
                    </List.Item>
                  )}
                />
              ))}
            </Space>
          )}
        </div>

        <div className={styles.detail} role="region" aria-label={t('skills.preview')}>
          {renderSkillDetail()}
        </div>
      </div>
    </Space>
  );
}
