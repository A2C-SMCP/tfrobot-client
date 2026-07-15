import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { App, Alert, Button, Card, Empty, Form, Input, List, Modal, Skeleton, Space, Tag, Typography } from 'antd';
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  EditOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  formatInvokeError,
  useSkillStore,
  type MarketplaceSummary,
  type PluginSummary,
  type SkillRef,
  type SkillResource,
} from '@/stores/skillStore';
import styles from './MarketplaceTab.module.css';

const { Text, Title, Paragraph } = Typography;
const EMPTY_MARKETPLACES: MarketplaceSummary[] = [];
const EMPTY_PLUGINS: PluginSummary[] = [];
const EMPTY_SKILLS: SkillRef[] = [];

interface MarketplaceTabProps {
  instanceId: string;
}

interface MarketplaceFormValues {
  name: string;
  gitUrl: string;
}

function pluginKey(plugin: Pick<PluginSummary, 'marketplace' | 'plugin'>) {
  return `${plugin.marketplace}/${plugin.plugin}`;
}

function extractLastUpdated(message?: string | null) {
  const match = message?.match(/lastUpdated=([^\s]+)/);
  return match?.[1] ?? null;
}

function renderMarkdown(markdown: string) {
  const blocks = markdown.split(/\n{2,}/);

  return blocks.map((block, index) => {
    const trimmed = block.trim();
    if (!trimmed) return null;
    if (trimmed.startsWith('```')) {
      return (
        <pre key={index} className={styles.previewCode}>
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
        <ul key={index} className={styles.previewList}>
          {trimmed.split('\n').map((line) => (
            <li key={line}>{line.replace(/^[-*] /, '')}</li>
          ))}
        </ul>
      );
    }
    return <Paragraph key={index} className={styles.previewParagraph}>{trimmed}</Paragraph>;
  });
}

export function MarketplaceTab({ instanceId }: MarketplaceTabProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [marketplaceForm] = Form.useForm<MarketplaceFormValues>();
  const [editingMarketplace, setEditingMarketplace] = useState<string | null>(null);
  const [marketplaceModalOpen, setMarketplaceModalOpen] = useState(false);
  const [selectedMarketplaceName, setSelectedMarketplaceName] = useState<string | null>(null);
  const [selectedPluginKey, setSelectedPluginKey] = useState<string | null>(null);
  const [selectedSkillName, setSelectedSkillName] = useState<string | null>(null);
  const [skillPreview, setSkillPreview] = useState<SkillResource | null>(null);
  const [loadingSkillPreview, setLoadingSkillPreview] = useState(false);
  const [skillPreviewError, setSkillPreviewError] = useState<string | null>(null);
  const {
    recordsByInstanceId,
    fetchMarketplaceGovernance,
    fetchSkills,
    addMarketplace,
    updateMarketplace,
    removeMarketplace,
    installPlugin,
    enablePlugin,
    disablePlugin,
    uninstallPlugin,
  } = useSkillStore();
  const governance = recordsByInstanceId[instanceId]?.governance ?? null;
  const capabilities = governance?.capabilities ?? null;
  const marketplaces = governance?.marketplaces ?? EMPTY_MARKETPLACES;
  const plugins = governance?.plugins ?? EMPTY_PLUGINS;
  const recordSkills = recordsByInstanceId[instanceId]?.skills;
  const skills = Array.isArray(recordSkills) ? recordSkills : EMPTY_SKILLS;
  const loadingMarketplace = recordsByInstanceId[instanceId]?.loadingMarketplace ?? false;
  const marketplaceError = recordsByInstanceId[instanceId]?.marketplaceError ?? null;

  useEffect(() => {
    fetchMarketplaceGovernance(instanceId);
    fetchSkills(instanceId);
  }, [fetchMarketplaceGovernance, fetchSkills, instanceId]);

  useEffect(() => {
    if (marketplaces.length === 0) {
      setSelectedMarketplaceName(null);
      return;
    }
    if (!selectedMarketplaceName || !marketplaces.some((marketplace) => marketplace.name === selectedMarketplaceName)) {
      setSelectedMarketplaceName(marketplaces[0].name);
    }
  }, [marketplaces, selectedMarketplaceName]);

  const marketplacePlugins = useMemo(() => (
    selectedMarketplaceName
      ? plugins.filter((plugin) => plugin.marketplace === selectedMarketplaceName)
      : []
  ), [plugins, selectedMarketplaceName]);

  useEffect(() => {
    if (marketplacePlugins.length === 0) {
      setSelectedPluginKey(null);
      setSelectedSkillName(null);
      setSkillPreview(null);
      setSkillPreviewError(null);
      return;
    }
    if (!selectedPluginKey || !marketplacePlugins.some((plugin) => pluginKey(plugin) === selectedPluginKey)) {
      setSelectedPluginKey(pluginKey(marketplacePlugins[0]));
      setSelectedSkillName(null);
      setSkillPreview(null);
      setSkillPreviewError(null);
    }
  }, [marketplacePlugins, selectedPluginKey]);

  const selectedMarketplace = marketplaces.find((marketplace) => marketplace.name === selectedMarketplaceName) ?? null;
  const selectedPlugin = marketplacePlugins.find((plugin) => pluginKey(plugin) === selectedPluginKey) ?? null;
  const selectedPluginSkills = selectedPlugin
    ? selectedPlugin.installed
      ? selectedPlugin.bundledSkills
      : selectedPlugin.declared?.skills ?? null
    : null;
  const selectedPluginMcpServers = selectedPlugin
    ? selectedPlugin.installed
      ? selectedPlugin.bundledMcpServers
      : selectedPlugin.declared?.mcpServers ?? null
    : null;
  const skillDescriptionByName = useMemo(() => (
    new Map(skills.map((skill) => [skill.name, skill.description]))
  ), [skills]);
  const selectedMarketplaceLastUpdated = extractLastUpdated(selectedMarketplace?.message);
  const supported = capabilities?.computerLifecycleApiAvailable ?? false;
  const canRunOperation = (operation: string) => {
    if (!supported) return false;
    return capabilities?.supportedOperations.includes(operation) ?? false;
  };

  const clearSkillPreview = () => {
    setSelectedSkillName(null);
    setSkillPreview(null);
    setSkillPreviewError(null);
    setLoadingSkillPreview(false);
  };

  const selectMarketplace = (name: string) => {
    setSelectedMarketplaceName(name);
    setSelectedPluginKey(null);
    clearSkillPreview();
  };

  const selectPlugin = (plugin: PluginSummary) => {
    setSelectedPluginKey(pluginKey(plugin));
    clearSkillPreview();
  };

  const handleOpenAddMarketplace = () => {
    setEditingMarketplace(null);
    marketplaceForm.resetFields();
    setMarketplaceModalOpen(true);
  };

  const handleSubmitMarketplace = async () => {
    const values = await marketplaceForm.validateFields();
    try {
      if (editingMarketplace) {
        await updateMarketplace(instanceId, { ...values, name: editingMarketplace });
        setSelectedMarketplaceName(editingMarketplace);
      } else {
        await addMarketplace(instanceId, values);
        setSelectedMarketplaceName(values.name);
      }
      marketplaceForm.resetFields();
      setEditingMarketplace(null);
      setMarketplaceModalOpen(false);
      message.success(t(editingMarketplace ? 'marketplace.messages.updated' : 'marketplace.messages.added'));
    } catch (e) {
      message.error(formatInvokeError(e));
    }
  };

  const handleEditMarketplace = (marketplace: MarketplaceFormValues) => {
    setEditingMarketplace(marketplace.name);
    setSelectedMarketplaceName(marketplace.name);
    marketplaceForm.setFieldsValue(marketplace);
    setMarketplaceModalOpen(true);
  };

  const handleCancelEditMarketplace = () => {
    setEditingMarketplace(null);
    marketplaceForm.resetFields();
    setMarketplaceModalOpen(false);
  };

  const handleRefresh = async () => {
    await fetchMarketplaceGovernance(instanceId);
    await fetchSkills(instanceId);
  };

  const handlePluginAction = async (
    action: 'install' | 'enable' | 'disable' | 'uninstall',
    plugin: PluginSummary,
  ) => {
    const request = { marketplace: plugin.marketplace, plugin: plugin.plugin };
    try {
      if (action === 'install') await installPlugin(instanceId, request);
      if (action === 'enable') await enablePlugin(instanceId, request);
      if (action === 'disable') await disablePlugin(instanceId, request);
      if (action === 'uninstall') await uninstallPlugin(instanceId, request);
      message.success(t(`marketplace.messages.${action}`));
    } catch (e) {
      message.error(formatInvokeError(e));
    }
  };

  const handlePreviewSkill = async (name: string) => {
    setSelectedSkillName(name);
    setSkillPreview(null);
    setSkillPreviewError(null);
    setLoadingSkillPreview(true);
    try {
      const resource = await invoke<SkillResource>('get_skill', {
        instanceId,
        name,
        relPath: null,
      });
      setSkillPreview(resource);
    } catch (e) {
      setSkillPreviewError(formatInvokeError(e));
    } finally {
      setLoadingSkillPreview(false);
    }
  };

  const renderPluginActions = (plugin: PluginSummary) => (
    <Space size="small" wrap>
      {plugin.status === 'available' ? (
        <Button
          size="small"
          icon={<CloudDownloadOutlined />}
          disabled={!canRunOperation('install_plugin')}
          loading={loadingMarketplace}
          onClick={(event) => {
            event.stopPropagation();
            handlePluginAction('install', plugin);
          }}
        >
          {t('marketplace.actions.installPlugin')}
        </Button>
      ) : (
        <Button
          size="small"
          icon={<PlayCircleOutlined />}
          disabled={!canRunOperation('enable_plugin') || plugin.enabled}
          loading={loadingMarketplace}
          onClick={(event) => {
            event.stopPropagation();
            handlePluginAction('enable', plugin);
          }}
        >
          {t('marketplace.actions.enablePlugin')}
        </Button>
      )}
      {plugin.status !== 'available' && (
        <Button
          size="small"
          icon={<PauseCircleOutlined />}
          disabled={!canRunOperation('disable_plugin') || !plugin.enabled}
          loading={loadingMarketplace}
          onClick={(event) => {
            event.stopPropagation();
            handlePluginAction('disable', plugin);
          }}
        >
          {t('marketplace.actions.disablePlugin')}
        </Button>
      )}
      {plugin.status !== 'available' && (
        <Button
          size="small"
          danger
          icon={<DeleteOutlined />}
          disabled={!canRunOperation('uninstall_plugin')}
          loading={loadingMarketplace}
          onClick={(event) => {
            event.stopPropagation();
            handlePluginAction('uninstall', plugin);
          }}
        >
          {t('marketplace.actions.uninstallPlugin')}
        </Button>
      )}
    </Space>
  );

  const renderSkillPreview = () => {
    if (!selectedSkillName) {
      return <Empty description={t('marketplace.details.selectSkill')} image={Empty.PRESENTED_IMAGE_SIMPLE} />;
    }
    if (loadingSkillPreview) {
      return <Skeleton active paragraph={{ rows: 8 }} />;
    }
    if (skillPreviewError) {
      return <Alert type="error" showIcon message={t('marketplace.details.skillUnavailable')} description={skillPreviewError} />;
    }
    if (!skillPreview?.body) {
      return <Alert type="warning" showIcon message={t('marketplace.details.skillUnavailable')} description={t('marketplace.details.skillPreviewUnavailable')} />;
    }
    return <div className={styles.skillPreview}>{renderMarkdown(skillPreview.body)}</div>;
  };

  const renderAssetCard = (
    name: string,
    description: string,
    options?: { selected?: boolean; onClick?: () => void },
  ) => {
    const content = (
      <>
        <Text strong ellipsis className={styles.assetName}>{name}</Text>
        <Text type="secondary" ellipsis className={styles.assetDescription}>{description}</Text>
      </>
    );
    if (!options?.onClick) {
      return (
        <div key={name} className={styles.assetCard}>
          {content}
        </div>
      );
    }
    return (
      <button
        key={name}
        type="button"
        className={options.selected ? styles.selectedAssetCard : styles.assetCard}
        onClick={options.onClick}
      >
        {content}
      </button>
    );
  };

  return (
    <Space direction="vertical" size={16} className={styles.root}>
      <div className={styles.header}>
        <Space direction="vertical" size={2}>
          <Title level={4} className={styles.title}>{t('marketplace.title')}</Title>
          {selectedMarketplaceLastUpdated && (
            <Text type="secondary" className={styles.lastUpdated}>
              {t('marketplace.lastUpdated', { time: selectedMarketplaceLastUpdated })}
            </Text>
          )}
        </Space>
        <Button
          icon={<ReloadOutlined />}
          loading={loadingMarketplace}
          onClick={handleRefresh}
        >
          {t('common.refresh')}
        </Button>
      </div>

      {marketplaceError && (
        <Alert type="error" showIcon message={t('common.error')} description={marketplaceError} />
      )}

      <div className={styles.columns}>
        <Card
          size="small"
          title={t('marketplace.marketplacesTitle')}
          className={styles.columnCard}
          extra={(
            <Button
              size="small"
              type="primary"
              icon={<PlusOutlined />}
              disabled={!canRunOperation('add_marketplace')}
              onClick={handleOpenAddMarketplace}
            >
              {t('common.add')}
            </Button>
          )}
        >
          <Space direction="vertical" size={16} className={styles.columnContent}>
            {marketplaces.length === 0 ? (
              <Empty description={t('marketplace.emptyMarketplaces')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
            ) : (
              <List
                size="small"
                dataSource={marketplaces}
                className={styles.scrollList}
                renderItem={(marketplace) => (
                  <List.Item
                    className={selectedMarketplaceName === marketplace.name ? styles.selectedItem : styles.selectableItem}
                    onClick={() => selectMarketplace(marketplace.name)}
                  >
                    <div className={styles.marketplaceRow}>
                      <div className={styles.marketplaceInfo}>
                        <Space className={styles.marketplaceTitle}>
                          <Text strong ellipsis className={styles.marketplaceName}>{marketplace.name}</Text>
                          <Tag>{marketplace.status}</Tag>
                        </Space>
                        <Text type="secondary" ellipsis className={styles.marketplaceUrl}>
                          {marketplace.gitUrl ?? t('marketplace.sdkOwnedState')}
                        </Text>
                      </div>
                      <Space size={4} className={styles.marketplaceActions}>
                        <Button
                          size="small"
                          type="text"
                          icon={<EditOutlined />}
                          aria-label={t('common.edit')}
                          title={t('common.edit')}
                          disabled={!canRunOperation('update_marketplace')}
                          loading={loadingMarketplace}
                          onClick={(event) => {
                            event.stopPropagation();
                            handleEditMarketplace({ name: marketplace.name, gitUrl: marketplace.gitUrl ?? '' });
                          }}
                        />
                        <Button
                          size="small"
                          type="text"
                          danger
                          icon={<DeleteOutlined />}
                          aria-label={t('marketplace.actions.removeMarketplace')}
                          title={t('marketplace.actions.removeMarketplace')}
                          disabled={!canRunOperation('remove_marketplace')}
                          loading={loadingMarketplace}
                          onClick={(event) => {
                            event.stopPropagation();
                            removeMarketplace(instanceId, marketplace.name);
                          }}
                        />
                      </Space>
                    </div>
                  </List.Item>
                )}
              />
            )}
          </Space>
        </Card>

        <Card
          size="small"
          title={selectedMarketplace ? t('marketplace.pluginsForMarketplace', { marketplace: selectedMarketplace.name }) : t('marketplace.pluginsTitle')}
          className={styles.columnCard}
        >
          {!selectedMarketplace ? (
            <Empty description={t('marketplace.emptyMarketplaceSelection')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : marketplacePlugins.length === 0 ? (
            <Empty description={t('marketplace.emptyPlugins')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : (
            <List
              size="small"
              dataSource={marketplacePlugins}
              className={styles.scrollList}
              renderItem={(plugin) => (
                <List.Item
                  className={selectedPluginKey === pluginKey(plugin) ? styles.selectedItem : styles.selectableItem}
                  onClick={() => selectPlugin(plugin)}
                >
                  <Space direction="vertical" size={8} className={styles.pluginCardBody}>
                    <Space wrap>
                      <Text strong>{plugin.plugin}</Text>
                      {plugin.version && <Tag>{plugin.version}</Tag>}
                      <Tag color={plugin.enabled ? 'green' : undefined}>{plugin.status}</Tag>
                    </Space>
                    {plugin.message && <Text type="secondary">{plugin.message}</Text>}
                    {renderPluginActions(plugin)}
                  </Space>
                </List.Item>
              )}
            />
          )}
        </Card>

        <Card size="small" title={t('marketplace.details.title')} className={styles.columnCard}>
          {!selectedPlugin ? (
            <Empty description={t('marketplace.details.emptyPlugin')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : (
            <Space direction="vertical" size={16} className={styles.columnContent}>
              <Space direction="vertical" size={4}>
                <Space wrap>
                  <Text strong>{selectedPlugin.plugin}</Text>
                  <Tag>{selectedPlugin.marketplace}</Tag>
                  {selectedPlugin.version && <Tag>{selectedPlugin.version}</Tag>}
                  <Tag color={selectedPlugin.enabled ? 'green' : undefined}>{selectedPlugin.status}</Tag>
                </Space>
                {selectedPlugin.message && <Text type="secondary">{selectedPlugin.message}</Text>}
              </Space>

              <Space direction="vertical" size={8} className={styles.detailSection}>
                <Text strong>{t('marketplace.details.skills')}</Text>
                {selectedPluginSkills === null ? (
                  <Text type="secondary">{t('marketplace.details.unknownSkills')}</Text>
                ) : selectedPluginSkills.length === 0 ? (
                  <Text type="secondary">{t('marketplace.details.emptySkills')}</Text>
                ) : (
                  <div className={styles.assetGrid}>
                    {selectedPluginSkills.map((skill) => (
                      renderAssetCard(
                        skill,
                        skillDescriptionByName.get(skill) ?? t('marketplace.details.emptyDescription'),
                        {
                          selected: selectedSkillName === skill,
                          onClick: () => handlePreviewSkill(skill),
                        },
                      )
                    ))}
                  </div>
                )}
              </Space>

              <Space direction="vertical" size={8} className={styles.detailSection}>
                <Text strong>{t('marketplace.details.mcpServers')}</Text>
                {selectedPluginMcpServers === null ? (
                  <Text type="secondary">{t('marketplace.details.unknownMcpServers')}</Text>
                ) : selectedPluginMcpServers.length === 0 ? (
                  <Text type="secondary">{t('marketplace.details.emptyMcpServers')}</Text>
                ) : (
                  <div className={styles.assetGrid}>
                    {selectedPluginMcpServers.map((server) => (
                      renderAssetCard(server, t('marketplace.details.mcpServerDescription'))
                    ))}
                  </div>
                )}
              </Space>

              <div className={styles.previewPane}>
                {renderSkillPreview()}
              </div>
            </Space>
          )}
        </Card>
      </div>
      <Modal
        title={editingMarketplace ? t('marketplace.actions.updateMarketplace') : t('marketplace.actions.addMarketplace')}
        open={marketplaceModalOpen}
        okText={editingMarketplace ? t('common.update') : t('common.add')}
        cancelText={t('common.cancel')}
        confirmLoading={loadingMarketplace}
        okButtonProps={{
          disabled: !(editingMarketplace ? canRunOperation('update_marketplace') : canRunOperation('add_marketplace')),
        }}
        onOk={handleSubmitMarketplace}
        onCancel={handleCancelEditMarketplace}
        destroyOnHidden
      >
        <Form form={marketplaceForm} layout="vertical">
          <Form.Item
            name="name"
            label={t('marketplace.form.name')}
            rules={[{ required: true, whitespace: true, message: t('marketplace.form.nameRequired') }]}
          >
            <Input disabled={!!editingMarketplace || !canRunOperation('add_marketplace')} />
          </Form.Item>
          <Form.Item
            name="gitUrl"
            label={t('marketplace.form.gitUrl')}
            rules={[{ required: true, whitespace: true, message: t('marketplace.form.gitUrlRequired') }]}
          >
            <Input disabled={!(editingMarketplace ? canRunOperation('update_marketplace') : canRunOperation('add_marketplace'))} />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
}
