import { usePageActive, usePageAction } from '@/components/Navigation/pageActivityState';
import { useNavigationForm, useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { App, Alert, Button, Card, Empty, Form, Input, Dropdown, List, Radio, Select, Skeleton, Space, Tabs, Tag, Typography } from 'antd';
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  FolderOpenOutlined,
  LoadingOutlined,
  PauseCircleOutlined,
  SearchOutlined,
  SettingOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import {
  formatInvokeError,
  useSkillStore,
  type MarketplaceSummary,
  type MarketplaceRemovalPlan,
  type MarketplaceSource,
  type PluginSummary,
  type SkillRef,
  type SkillResource,
} from '@/stores/skillStore';
import { useComputerStore } from '@/stores/computerStore';
import { useConfigAdoptionStore } from '@/stores/configAdoptionStore';
import { isRuntimeInputCancelledError } from '@/utils/runtimeActionError';
import styles from './MarketplaceTab.module.css';

const { Text, Title, Paragraph } = Typography;
const EMPTY_MARKETPLACES: MarketplaceSummary[] = [];
const EMPTY_PLUGINS: PluginSummary[] = [];
const EMPTY_SKILLS: SkillRef[] = [];

interface MarketplaceTabProps {
  instanceId: string;
  onOpenRuntime?: () => void;
  targetPlugin?: Pick<PluginSummary, 'marketplace' | 'plugin' | 'pluginId'> | null;
  onTargetPluginConsumed?: () => void;
}

interface MarketplaceFormValues {
  source: MarketplaceSource;
}

function pluginKey(plugin: Pick<PluginSummary, 'marketplace' | 'plugin' | 'pluginId'>) {
  return `${plugin.marketplace}/${plugin.plugin}/${plugin.pluginId ?? ''}`;
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

export function MarketplaceTab({
  instanceId,
  onOpenRuntime,
  targetPlugin,
  onTargetPluginConsumed,
}: MarketplaceTabProps) {
  const pageActive = usePageActive();
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [marketplaceForm] = Form.useForm<MarketplaceFormValues>();
  const [marketplaceModalOpen, setMarketplaceModalOpen] = useNavigationState('marketplace.editorOpen', false);
  const [selectedMarketplaceName, setSelectedMarketplaceName] = useNavigationState<string | null>('marketplace.selected', null);
  const [selectedPluginKey, setSelectedPluginKey] = useNavigationState<string | null>('marketplace.plugin', null);
  const [view, setView] = useNavigationState<'installed' | 'discover'>('marketplace.view', 'installed');
  const [search, setSearch] = useNavigationState('marketplace.search', '');
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [removalSource, setRemovalSource] = useState<string | null>(null);
  const [removalPlan, setRemovalPlan] = useState<MarketplaceRemovalPlan | null>(null);
  const [removalError, setRemovalError] = useState<string | null>(null);
  const [loadingRemoval, setLoadingRemoval] = useState(false);
  const [removingSource, setRemovingSource] = useState(false);
  const removalRequest = useRef(0);
  const removalInFlight = useRef(false);
  const [pendingUninstall, setPendingUninstall] = useState<PluginSummary | null>(null);
  const [operationError, setOperationError] = useState<{ key: string; action: 'install' | 'enable' | 'disable' | 'uninstall'; message: string } | null>(null);
  const instance = useComputerStore((state) => state.instances.find((item) => item.id === instanceId));
  const storageChange = useConfigAdoptionStore((state) => state.statuses[instanceId]?.changes.find((change) => change.reason === 'skill_home'));
  const [selectedSkillName, setSelectedSkillName] = useNavigationState<string | null>('marketplace.skill', null);
  const previewKey = useRef<string | null>(null);
  const [skillPreview, setSkillPreview] = useState<SkillResource | null>(null);
  const [loadingSkillPreview, setLoadingSkillPreview] = useState(false);
  const [skillPreviewError, setSkillPreviewError] = useState<string | null>(null);
  const currentInstanceIdRef = useRef<string | null>(instanceId);
  const {
    recordsByInstanceId,
    fetchMarketplaceGovernance,
    fetchSkills,
    addMarketplace,
    refreshMarketplace,
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
  const marketplaceOperation = recordsByInstanceId[instanceId]?.marketplaceOperation ?? null;
  const marketplaceBusy = loadingMarketplace || marketplaceOperation !== null;
  const marketplaceError = recordsByInstanceId[instanceId]?.marketplaceError ?? null;
  const pageAction = usePageAction(instanceId);
  const marketplaceDraft = useNavigationForm('marketplace.form', marketplaceForm, {} as MarketplaceFormValues);
  const marketplaceSourceType = Form.useWatch(['source', 'type'], marketplaceForm) ?? 'remoteGit';

  useEffect(() => {
    if (!pageActive) { setSourcesOpen(false); setPendingUninstall(null); setRemovalSource(null); removalRequest.current += 1; removalInFlight.current = false; setRemovingSource(false); setLoadingRemoval(false); }
  }, [pageActive]);

  useLayoutEffect(() => {
    currentInstanceIdRef.current = instanceId;
    removalRequest.current += 1;
    removalInFlight.current = false;
    setRemovalSource(null);
    setRemovalPlan(null);
    setRemovalError(null);
    setLoadingRemoval(false);
    setRemovingSource(false);
    setOperationError(null);
    setPendingUninstall(null);
    setSourcesOpen(false);
    return () => {
      currentInstanceIdRef.current = null;
    };
  }, [instanceId]);

  useEffect(() => {
    if (!pageActive) return;
    fetchMarketplaceGovernance(instanceId);
    fetchSkills(instanceId);
  }, [fetchMarketplaceGovernance, fetchSkills, instanceId, pageActive]);

  useEffect(() => {
    if (selectedMarketplaceName && !marketplaces.some((source) => source.name === selectedMarketplaceName)) setSelectedMarketplaceName(null);
  }, [marketplaces, selectedMarketplaceName, setSelectedMarketplaceName]);

  const marketplacePlugins = useMemo(() => {
    const query = search.trim().toLowerCase();
    return plugins.filter((plugin) => (view !== 'installed' || plugin.installed)
      && (view !== 'discover' || !selectedMarketplaceName || plugin.marketplace === selectedMarketplaceName)
      && (!query || [plugin.plugin, plugin.marketplace, plugin.declared?.description ?? ''].some((value) => value.toLowerCase().includes(query))));
  }, [plugins, view, search, selectedMarketplaceName]);

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
  }, [marketplacePlugins, selectedPluginKey, setSelectedPluginKey, setSelectedSkillName]);

  useEffect(() => {
    if (!targetPlugin) return;
    const target = plugins.find((plugin) => (
      plugin.marketplace === targetPlugin.marketplace
      && plugin.plugin === targetPlugin.plugin
      && (targetPlugin.pluginId
        ? plugin.pluginId === targetPlugin.pluginId
        : true)
    ));
    if (!target) return;
    setView(target.installed ? 'installed' : 'discover');
    setSearch('');
    setSelectedMarketplaceName(null);
    setSelectedPluginKey(pluginKey(target));
    setSelectedSkillName(null);
    setSkillPreview(null);
    setSkillPreviewError(null);
    onTargetPluginConsumed?.();
  }, [onTargetPluginConsumed, plugins, targetPlugin, setSelectedMarketplaceName, setSelectedPluginKey, setSelectedSkillName, setView, setSearch]);

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
  const supported = capabilities?.computerLifecycleApiAvailable ?? false;
  const canRunOperation = (operation: string) => {
    if (!supported) return false;
    return capabilities?.supportedOperations.includes(operation) ?? false;
  };

  const loadRemovalPlan = async (marketplace: string) => {
    const current = pageAction();
    const request = ++removalRequest.current;
    setRemovalSource(marketplace);
    setRemovalPlan(null);
    setRemovalError(null);
    setLoadingRemoval(true);
    try {
      const plan = await invoke<MarketplaceRemovalPlan>('get_marketplace_removal_plan', { instanceId, marketplace });
      if (current() && removalRequest.current === request) setRemovalPlan(plan);
    } catch (error) {
      if (current() && removalRequest.current === request) setRemovalError(formatInvokeError(error));
    } finally {
      if (current() && removalRequest.current === request) setLoadingRemoval(false);
    }
  };
  const cancelRemoval = () => {
    if (removalInFlight.current) return;
    removalRequest.current += 1;
    setRemovalSource(null);
    setRemovalPlan(null);
  };
  const confirmRemoval = async () => {
    if (!removalPlan || removalInFlight.current || marketplaceBusy) return;
    const current = pageAction();
    const request = removalRequest.current;
    removalInFlight.current = true;
    setRemovingSource(true);
    setRemovalError(null);
    try {
      await removeMarketplace(instanceId, removalPlan.marketplace, removalPlan);
      if (current() && removalRequest.current === request) {
        setRemovalSource(null);
        setRemovalPlan(null);
      }
    } catch (error) {
      if (current() && removalRequest.current === request) {
        setRemovalPlan(null);
        setRemovalError(formatInvokeError(error));
      }
    } finally {
      if (removalRequest.current === request) {
        removalInFlight.current = false;
        setRemovingSource(false);
      }
    }
  };

  const clearSkillPreview = () => {
    setSelectedSkillName(null);
    setSkillPreview(null);
    setSkillPreviewError(null);
    setLoadingSkillPreview(false);
  };

  const selectMarketplace = (name: string | null) => {
    setSelectedMarketplaceName(name);
    setSelectedPluginKey(null);
    clearSkillPreview();
  };

  const selectPlugin = (plugin: PluginSummary) => {
    setSelectedPluginKey(pluginKey(plugin));
    clearSkillPreview();
  };

  const handleOpenAddMarketplace = () => {
    marketplaceDraft.clearDraft();
    marketplaceForm.resetFields();
    marketplaceDraft.setValues({
      source: { type: 'remoteGit', gitUrl: '' },
    });
    setMarketplaceModalOpen(true);
  };

  const handleSubmitMarketplace = async () => {
    const current = pageAction();
    let values: MarketplaceFormValues;
    try {
      values = await marketplaceForm.validateFields();
    } catch {
      return;
    }
    if (!current()) return;
    const request = {
      source: values.source.type === 'localGit'
        ? { type: 'localGit' as const, path: values.source.path }
        : { type: 'remoteGit' as const, gitUrl: values.source.gitUrl },
    };
    try {
      await addMarketplace(instanceId, request);
      if (currentInstanceIdRef.current !== instanceId) return;
      if (!current()) return;
      marketplaceDraft.clearDraft();
      marketplaceForm.resetFields();
      setMarketplaceModalOpen(false);
      setSelectedMarketplaceName(null);
      message.success(t('marketplace.messages.added'));
    } catch (e) {
      if (currentInstanceIdRef.current !== instanceId) return;
      message.error(formatInvokeError(e));
    }
  };

  const handleChooseMarketplaceDirectory = async () => {
    const current = pageAction();
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      if (!current()) return;
      const path = await open({ directory: true, multiple: false });
      if (path && current()) {
        marketplaceDraft.setValues({ source: { type: 'localGit', path } });
      }
    } catch (error) {
      message.error(formatInvokeError(error));
    }
  };

  const handleCancelAddMarketplace = () => {
    marketplaceDraft.clearDraft();
    marketplaceForm.resetFields();
    setMarketplaceModalOpen(false);
  };

  const handleRefresh = async () => {
    setOperationError(null);
    const current = pageAction();
    await fetchMarketplaceGovernance(instanceId);
    if (current()) await fetchSkills(instanceId);
  };

  const handleRefreshMarketplace = async (marketplace: string) => {
    try {
      await refreshMarketplace(instanceId, marketplace);
      if (currentInstanceIdRef.current !== instanceId) return;
      message.success(t('marketplace.messages.refreshed'));
    } catch (error) {
      if (currentInstanceIdRef.current !== instanceId) return;
      message.error(formatInvokeError(error));
    }
  };

  const handlePluginAction = async (
    action: 'install' | 'enable' | 'disable' | 'uninstall',
    plugin: PluginSummary,
  ) => {
    const request = { marketplace: plugin.marketplace, plugin: plugin.plugin };
    setOperationError(null);
    try {
      if (action === 'install') await installPlugin(instanceId, request);
      if (action === 'enable') await enablePlugin(instanceId, request);
      if (action === 'disable') await disablePlugin(instanceId, request);
      if (action === 'uninstall') await uninstallPlugin(instanceId, request);
      if (currentInstanceIdRef.current !== instanceId) return;
      message.success(t(`marketplace.messages.${action}`));
    } catch (e) {
      if (currentInstanceIdRef.current !== instanceId) return;
      if (isRuntimeInputCancelledError(e)) return;
      setOperationError({ key: pluginKey(plugin), action, message: formatInvokeError(e) });
    }
  };

  useEffect(() => {
    if (!pageActive || !selectedSkillName || !selectedPlugin?.installed || !selectedPlugin.enabled) return;
    let cancelled = false;
    const key = JSON.stringify([instanceId, selectedSkillName]);
    if (previewKey.current !== key) setSkillPreview(null);
    setSkillPreviewError(null);
    setLoadingSkillPreview(true);
    void invoke<SkillResource>('get_skill', {
      instanceId, name: selectedSkillName, relPath: null,
    }).then((resource) => {
      if (!cancelled) { previewKey.current = key; setSkillPreview(resource); }
    }).catch((error) => {
      if (!cancelled) setSkillPreviewError(formatInvokeError(error));
    }).finally(() => {
      if (!cancelled) setLoadingSkillPreview(false);
    });
    return () => { cancelled = true; };
  }, [instanceId, pageActive, selectedSkillName, selectedPlugin?.installed, selectedPlugin?.enabled, selectedPlugin?.version]);

  const pluginStatus = (plugin: PluginSummary) => {
    if (plugin.status === 'degraded' || plugin.status === 'unknown') return t(`marketplace.status.${plugin.status}`);
    return t(`marketplace.status.${!plugin.installed ? 'available' : plugin.enabled ? 'enabled' : 'disabled'}`);
  };
  const renderPluginActions = (plugin: PluginSummary) => {
    const action = !plugin.installed ? 'install' : plugin.enabled ? 'disable' : 'enable';
    const icons = { install: <CloudDownloadOutlined />, enable: <PlayCircleOutlined />, disable: <PauseCircleOutlined /> };
    return <Space wrap>
      <Button aria-label={t(`marketplace.actions.${action}Plugin`)} type="primary" icon={icons[action]} disabled={marketplaceBusy || !canRunOperation(`${action}_plugin`)} loading={loadingMarketplace}
        onClick={() => void handlePluginAction(action, plugin)}>{t(`marketplace.actions.${action}Plugin`)}</Button>
      {plugin.installed && <Dropdown trigger={['click']} menu={{ items: [{ key: 'uninstall', danger: true, label: t('marketplace.actions.uninstallPlugin'), disabled: marketplaceBusy || !canRunOperation('uninstall_plugin'), onClick: () => setPendingUninstall(plugin) }] }}>
        <Button type="text" disabled={marketplaceBusy}>{t('marketplace.more')}</Button>
      </Dropdown>}
    </Space>;
  };

  const renderSkillPreview = () => {
    if (!selectedSkillName) {
      return <Empty description={t('marketplace.details.selectSkill')} image={Empty.PRESENTED_IMAGE_SIMPLE} />;
    }
    if (loadingSkillPreview && !skillPreview) {
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

  const operationStatus = marketplaceOperation && <Alert type="info" showIcon icon={<LoadingOutlined spin />}
        message={t(marketplaceOperation.kind === 'add' && !marketplaceOperation.target
          ? 'marketplace.operations.addUnnamed'
          : `marketplace.operations.${marketplaceOperation.kind}`, { target: marketplaceOperation.target })}
        description={t(marketplaceOperation.kind === 'add' || marketplaceOperation.kind === 'update' ? 'marketplace.operations.cloneDescription' : 'marketplace.operations.defaultDescription')} />;

  return (
    <Space direction="vertical" size={16} className={styles.root}>
      <div className={styles.header}>
        <Text type="secondary">{t('marketplace.context', { name: instance?.name ?? instanceId })}</Text>
        <Space wrap>
          <Button aria-label={t('marketplace.reloadList')} icon={<ReloadOutlined />} loading={loadingMarketplace} disabled={marketplaceBusy} onClick={handleRefresh}>{t('marketplace.reloadList')}</Button>
          <Button aria-label={t('marketplace.manageSources')} icon={<SettingOutlined />} onClick={() => setSourcesOpen(true)}>{t('marketplace.manageSources')}</Button>
        </Space>
      </div>
      {storageChange && <Alert type="warning" showIcon message={t('marketplace.storagePending')}
        description={t('marketplace.storagePendingDescription')}
        action={onOpenRuntime && <Button onClick={onOpenRuntime}>{t('computer.adoption.openRuntime')}</Button>} />}
      {marketplaceError && !operationError && !sourcesOpen && <Alert type="error" showIcon message={t('marketplace.loadOrOperationFailed')} description={marketplaceError} />}
      {operationError && <Alert type="error" showIcon message={t(`marketplace.errors.${operationError.action}`)} description={operationError.message}
        action={selectedPlugin && operationError.key === pluginKey(selectedPlugin) && <Button disabled={marketplaceBusy || !canRunOperation(`${operationError.action}_plugin`)} onClick={() => void handlePluginAction(operationError.action, selectedPlugin)}>{t('marketplace.retryOperation')}</Button>} />}
      {!sourcesOpen && operationStatus}

      <Tabs activeKey={view} onChange={(key) => { setView(key as 'installed' | 'discover'); clearSkillPreview(); }} items={[
        { key: 'installed', label: t('marketplace.installedTab', { count: plugins.filter((plugin) => plugin.installed).length }) },
        { key: 'discover', label: t('marketplace.discoverTab') },
      ]} />
      <div className={styles.filters}>
        <Input allowClear prefix={<SearchOutlined />} aria-label={t('marketplace.search')} placeholder={t('marketplace.search')} value={search} onChange={(event) => setSearch(event.target.value)} />
        {view === 'discover' && <Select aria-label={t('marketplace.filterSource')} value={selectedMarketplaceName ?? ''}
          onChange={(value) => selectMarketplace(value || null)} options={[{ value: '', label: t('marketplace.allSources') }, ...marketplaces.map((source) => ({ value: source.name, label: source.name }))]} />}
      </div>
      <div className={styles.columns}>
        <Card size="small" title={t(view === 'installed' ? 'marketplace.installedList' : 'marketplace.discoverTab')} className={styles.columnCard}>
          {loadingMarketplace && !governance ? <Skeleton active /> : marketplacePlugins.length === 0 ? <Empty image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t(search.trim() || (view === 'discover' && selectedMarketplaceName) ? 'marketplace.noMatches' : view === 'installed' ? 'marketplace.noInstalled' : marketplaces.length ? 'marketplace.emptyPlugins' : 'marketplace.noSources')}>
            {view === 'installed' && !search.trim() ? <Button type="primary" onClick={() => setView('discover')}>{t('marketplace.discoverTab')}</Button> : view === 'discover' && !marketplaces.length && <Button disabled={!canRunOperation('add_marketplace') || marketplaceBusy} onClick={handleOpenAddMarketplace}>{t('marketplace.actions.addMarketplace')}</Button>}
          </Empty> : <List size="small" dataSource={marketplacePlugins} data-navigation-scroll="marketplace-plugins" className={styles.scrollList}
            renderItem={(plugin) => <List.Item className={selectedPluginKey === pluginKey(plugin) ? styles.selectedItem : styles.selectableItem}>
              <button type="button" className={styles.pluginSelectionButton}
                aria-label={t('marketplace.actions.selectPlugin', { plugin: plugin.plugin, pluginId: plugin.pluginId ?? plugin.plugin })}
                aria-pressed={selectedPluginKey === pluginKey(plugin)} onClick={() => selectPlugin(plugin)}>
                <Space wrap><Text strong>{plugin.plugin}</Text>{plugin.version && <Tag>{plugin.version}</Tag>}<Tag>{pluginStatus(plugin)}</Tag></Space>
                <Text type="secondary">{t('marketplace.fromSource', { name: plugin.marketplace })}</Text>
                {plugin.declared?.description && <Text type="secondary">{plugin.declared.description}</Text>}
              </button>
            </List.Item>} />}
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
                  <Tag>{pluginStatus(selectedPlugin)}</Tag>
                </Space>
                {selectedPlugin.message && <Text type="secondary">{selectedPlugin.message}</Text>}
              </Space>

              {renderPluginActions(selectedPlugin)}
              <Text type="secondary">{t(!selectedPlugin.installed ? 'marketplace.installHelp' : selectedPlugin.enabled ? 'marketplace.enabledHelp' : 'marketplace.disabledHelp')}</Text>
              <Text type="secondary">{t(selectedPlugin.installed ? 'marketplace.installedContents' : 'marketplace.declaredContents')}</Text>
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
                          onClick: selectedPlugin.installed && selectedPlugin.enabled ? () => setSelectedSkillName(skill) : undefined,
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

              {selectedPlugin.installed && selectedPlugin.enabled && !!selectedPluginSkills?.length && <div data-navigation-scroll="marketplace-preview" className={styles.previewPane}>
                {renderSkillPreview()}
              </div>}
            </Space>
          )}
        </Card>
      </div>
      <Modal title={t('marketplace.manageSources')} open={sourcesOpen} onCancel={() => setSourcesOpen(false)} width={760}
        footer={<Button onClick={() => setSourcesOpen(false)}>{t('common.close')}</Button>}>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <Text type="secondary">{t('marketplace.sourcesHelp')}</Text>
          {operationStatus}
          {marketplaceError && <Alert type="error" showIcon message={t('marketplace.loadOrOperationFailed')} description={marketplaceError} />}
          <Button aria-label={t('marketplace.actions.addMarketplace')} type="primary" icon={<PlusOutlined />} disabled={!canRunOperation('add_marketplace') || marketplaceBusy} onClick={handleOpenAddMarketplace}>{t('marketplace.actions.addMarketplace')}</Button>
          {!marketplaces.length ? <Empty description={t('marketplace.noSources')} image={Empty.PRESENTED_IMAGE_SIMPLE} /> :
            <List dataSource={marketplaces} renderItem={(source) => {
              const sourceStatus = ['known', 'available', 'degraded'].includes(source.status) ? source.status : 'unknown';
              return <List.Item><Space direction="vertical" style={{ width: '100%', minWidth: 0 }}>
                <Space wrap><Text strong>{source.name}</Text><Tag>{t(`marketplace.sourceStatus.${sourceStatus}`)}</Tag><Tag>{t(`marketplace.source.${source.source.type}`)}</Tag></Space>
                <Text type="secondary" className={styles.sourcePath}>{source.source.type === 'localGit' ? source.source.path : source.source.displayGitUrl ?? t('marketplace.sourceHidden')}</Text>
                {extractLastUpdated(source.message) && <Text type="secondary">{t('marketplace.lastUpdated', { time: extractLastUpdated(source.message) })}</Text>}
                {source.status === 'degraded' && source.message && <Text type="warning">{source.message}</Text>}
                <Space wrap>
                  <Button aria-label={t('marketplace.actions.refreshMarketplace')} size="small" icon={<ReloadOutlined />} disabled={marketplaceBusy || !canRunOperation('refresh_marketplace')} onClick={() => void handleRefreshMarketplace(source.name)}>{t('marketplace.actions.refreshMarketplace')}</Button>
                  <Button aria-label={t('marketplace.actions.removeMarketplace')} size="small" danger icon={<DeleteOutlined />} disabled={marketplaceBusy || !canRunOperation('remove_marketplace')} onClick={() => void loadRemovalPlan(source.name)}>{t('marketplace.actions.removeMarketplace')}</Button>
                </Space>
              </Space></List.Item>;
            }} />}
        </Space>
      </Modal>
      <Modal aria-label={t('marketplace.removeSourceConfirm', { name: removalSource })} title={t('marketplace.removeSourceConfirm', { name: removalSource })} open={removalSource !== null}
        okText={t(removalPlan?.installedPlugins.length ? 'marketplace.removal.confirmCascade' : 'marketplace.actions.removeMarketplace')}
        cancelText={t('common.cancel')} onCancel={cancelRemoval} onOk={() => void confirmRemoval()}
        confirmLoading={removingSource} closable={!removingSource} maskClosable={!removingSource} keyboard={!removingSource}
        cancelButtonProps={{ disabled: removingSource }} okButtonProps={{ danger: true, disabled: !removalPlan || loadingRemoval || marketplaceBusy || removingSource }}>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          {loadingRemoval && <Skeleton active paragraph={{ rows: 2 }} />}
          {removalError && <Alert type="error" showIcon message={t('marketplace.removal.failed')} description={removalError}
            action={<Button onClick={() => { if (removalSource) void loadRemovalPlan(removalSource); }} disabled={loadingRemoval || removingSource}>{t('marketplace.removal.reload')}</Button>} />}
          {removalPlan && (removalPlan.installedPlugins.length === 0 ?
            <Text>{t('marketplace.removal.empty')}</Text> : <>
              <Alert type="warning" showIcon message={t('marketplace.removal.warning')} description={t('marketplace.removal.description')} />
              <List size="small" dataSource={removalPlan.installedPlugins} renderItem={(plugin) => <List.Item>
                <Space direction="vertical" style={{ width: '100%' }}>
                  <Space wrap><Text strong>{plugin.plugin}</Text><Tag>{t(plugin.enabled ? 'marketplace.status.enabled' : 'marketplace.status.disabled')}</Tag></Space>
                  <Text className={styles.sourcePath}>{t('marketplace.details.skills')}: {plugin.bundledSkills.join(', ') || t('marketplace.details.emptySkills')}</Text>
                  <Text className={styles.sourcePath}>{t('marketplace.details.mcpServers')}: {plugin.bundledMcpServers.join(', ') || t('marketplace.details.emptyMcpServers')}</Text>
                </Space>
              </List.Item>} />
            </>)}
        </Space>
      </Modal>
      <Modal title={t('marketplace.uninstall.title', { name: pendingUninstall?.plugin })} open={!!pendingUninstall}
        okText={t('marketplace.actions.uninstallPlugin')} cancelText={t('common.cancel')}
        okButtonProps={{ danger: true, disabled: marketplaceBusy || !canRunOperation('uninstall_plugin') || !plugins.some((plugin) => pluginKey(plugin) === (pendingUninstall && pluginKey(pendingUninstall)) && plugin.installed) }}
        onCancel={() => setPendingUninstall(null)} onOk={() => { if (pendingUninstall) { void handlePluginAction('uninstall', pendingUninstall); setPendingUninstall(null); } }}>
        <Text>{t('marketplace.uninstall.description')}</Text>
      </Modal>

      <Modal
        title={t('marketplace.actions.addMarketplace')}
        open={marketplaceModalOpen && marketplaceOperation?.kind !== 'add' && marketplaceOperation?.kind !== 'update'}
        okText={t('marketplace.actions.addMarketplace')}
        cancelText={t('common.cancel')}
        confirmLoading={loadingMarketplace}
        okButtonProps={{
          disabled: marketplaceBusy || !canRunOperation('add_marketplace'),
        }}
        onOk={handleSubmitMarketplace}
        onCancel={handleCancelAddMarketplace}
        destroyOnHidden
      >
        <Form name={`marketplace-${instanceId}`} form={marketplaceForm} onValuesChange={(_, values) => marketplaceDraft.onValuesChange(values)} layout="vertical">
          <Form.Item
            name={['source', 'type']}
            label={t('marketplace.form.sourceType')}
          >
            <Radio.Group
              aria-label={t('marketplace.form.sourceType')}
              disabled={marketplaceBusy || !canRunOperation('add_marketplace')}
              options={[
                { value: 'remoteGit', label: t('marketplace.source.remoteGit') },
                { value: 'localGit', label: t('marketplace.source.localGit') },
              ]}
            />
          </Form.Item>
          {marketplaceSourceType === 'remoteGit' ? (
            <Form.Item
              name={['source', 'gitUrl']}
              label={t('marketplace.form.gitUrl')}
              rules={[
                { required: true, whitespace: true, message: t('marketplace.form.gitUrlRequired') },
                {
                  validator: async (_, value?: string) => {
                    if (value?.trim().toLowerCase().startsWith('file:')) {
                      throw new Error(t('marketplace.form.remoteFileUrlForbidden'));
                    }
                  },
                },
              ]}
            >
              <Input disabled={marketplaceBusy || !canRunOperation('add_marketplace')} />
            </Form.Item>
          ) : (
            <Form.Item label={t('marketplace.form.localPath')} required>
              <Space.Compact style={{ width: '100%' }}>
                <Form.Item
                  name={['source', 'path']}
                  noStyle
                  rules={[{ required: true, whitespace: true, message: t('marketplace.form.localPathRequired') }]}
                >
                  <Input
                    aria-label={t('marketplace.form.localPath')}
                    readOnly
                    disabled={marketplaceBusy || !canRunOperation('add_marketplace')}
                  />
                </Form.Item>
                <Button
                  aria-label={t('marketplace.actions.chooseFolder')}
                  icon={<FolderOpenOutlined />}
                  disabled={marketplaceBusy || !canRunOperation('add_marketplace')}
                  onClick={() => void handleChooseMarketplaceDirectory()}
                >
                  {t('marketplace.actions.chooseFolder')}
                </Button>
              </Space.Compact>
            </Form.Item>
          )}
        </Form>
      </Modal>
    </Space>
  );
}
