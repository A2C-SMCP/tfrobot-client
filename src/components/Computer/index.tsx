import { usePageAction, usePageActive } from '@/components/Navigation/pageActivityState';
import { PageHost } from '@/components/Navigation/PageHost';
import { NavigationScope } from '@/components/Navigation/NavigationMemory';
import { useNavigationForm, useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import { App, Button, Card, Dropdown, Empty, Form, Input, Select, Segmented, Skeleton, Space, Switch, Tag, Tooltip, Typography, type MenuProps } from 'antd';
import {
  AppstoreOutlined,
  BarsOutlined,
  CopyOutlined,
  DeleteOutlined,
  DisconnectOutlined,
  EditOutlined,
  EllipsisOutlined,
  LinkOutlined,
  PlayCircleOutlined,
  RightOutlined,
  SearchOutlined,
  SettingOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useEffect, useId, useRef, useState, type ComponentRef } from 'react';
import {
  useComputerStore,
  type ComputerInstance,
} from '@/stores/computerStore';
import { useManagerStore } from '@/stores/managerStore';
import { usePortableConfigStore } from '@/stores/portableConfigStore';
import { isRuntimeInputCancelledError } from '@/utils/runtimeActionError';
import { permissionHelpRoute } from '@/components/Settings/permissions';
import {
  computerSettingsNavigationKey,
  type ComputerWorkbenchSection,
} from './tabs';
import { ComputerWorkbench } from './ComputerWorkbench';
import { ComputerDeviceIcon } from './ComputerIdentity';
import { ConfigAdoptionStatus } from './ConfigAdoptionStatus';
import styles from './Computer.module.css';
import {
  computerStatusColor,
  connectDisabledReasonTranslationKey,
  resolveComputerConnection,
  usesStopAction,
} from './computerActions';

const { Title, Text } = Typography;

interface ComputerProps {
  targetMcpId?: string | null;
  navigationRevision?: number;
  initialView?: 'list' | 'detail';
  initialSection?: ComputerWorkbenchSection;
  onNavigate?: (key: string) => void;
}

async function copyComputerId(id: string) {
  try {
    await navigator.clipboard.writeText(id);
  } catch {
    // The desktop webview may not expose the async clipboard on its app origin.
    const previousFocus = document.activeElement;
    const field = document.createElement('textarea');
    field.value = id;
    field.readOnly = true;
    field.style.position = 'fixed';
    field.style.opacity = '0';
    document.body.append(field);
    try {
      field.select();
      if (!document.execCommand('copy')) throw new Error('Clipboard unavailable');
    } finally {
      field.remove();
      if (previousFocus instanceof HTMLElement) previousFocus.focus({ preventScroll: true });
    }
  }
}

function ComputerCard({
  instance,
  ambiguousName,
  onOpen,
  onOpenSettings,
  onEdit,
  onDuplicate,
  onStartStop,
  onConnect,
  onDisconnect,
  onDelete,
  loading,
}: {
  instance: ComputerInstance;
  ambiguousName: boolean;
  onOpen: () => void;
  onOpenSettings?: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onStartStop: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onDelete: () => Promise<void>;
  loading: boolean;
}) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const active = usePageActive();
  const menuTrigger = useRef<ComponentRef<typeof Button>>(null);
  const menuId = useId();
  const descriptionId = useId();
  const detailsHintId = useId();
  const nameWithId = ambiguousName ? `${instance.name} (${instance.id.slice(-8)})` : instance.name;
  const moreLabel = t('computer.collection.more', { name: nameWithId });
  const [menuOpen, setMenuOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  useEffect(() => {
    if (!active) {
      setMenuOpen(false);
      setDeleteOpen(false);
    }
  }, [active]);
  const currentManagerContext = useManagerStore((state) => state.context.contextKey);
  const stopAction = usesStopAction(instance.status);
  const startStopLabel = instance.status === 'starting'
    ? t('computer.runtime.actionProgress.starting')
    : instance.status === 'stopping'
      ? t('computer.runtime.actionProgress.stopping')
      : stopAction
        ? t('computer.stop')
        : t('computer.start');
  const startStopIcon = stopAction ? <StopOutlined /> : <PlayCircleOutlined />;
  const startStopCapability = stopAction
    ? instance.runtime.actions.stop
    : instance.runtime.actions.start;
  const canStartStop = startStopCapability.enabled;
  const startStopDisabledReason = startStopCapability.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${startStopCapability.disabled_reason}`)
    : undefined;
  const connection = resolveComputerConnection(instance, currentManagerContext);
  const disconnectDisabledReason = connection.actions.disconnect.disabled_reason
    ? t(`computer.connectionActions.disabledReasons.${connection.actions.disconnect.disabled_reason}`)
    : undefined;
  const connectDisabledReasonKey = connectDisabledReasonTranslationKey(connection);
  const connectDisabledReason = connectDisabledReasonKey
    ? t(connectDisabledReasonKey)
    : undefined;
  const menuItems: MenuProps['items'] = [
    onOpenSettings ? { key: 'settings', icon: <SettingOutlined aria-hidden="true" />, label: t('computer.redesign.settings') } : null,
    { key: 'edit', icon: <EditOutlined aria-hidden="true" />, label: t('computer.collection.editIdentity') },
    { key: 'copy-id', icon: <CopyOutlined aria-hidden="true" />, label: t('computer.workbench.actions.copyId') },
    { key: 'duplicate', icon: <CopyOutlined aria-hidden="true" />, label: t('computer.duplicate') },
    { type: 'divider' },
    { key: 'delete', icon: <DeleteOutlined aria-hidden="true" />, danger: true, label: t('computer.delete') },
  ];
  const handleMenuAction: MenuProps['onClick'] = async ({ key }) => {
    setMenuOpen(false);
    menuTrigger.current?.focus();
    switch (key) {
      case 'settings':
        onOpenSettings?.();
        break;
      case 'edit':
        onEdit();
        break;
      case 'copy-id':
        try {
          await copyComputerId(instance.id);
          message.success(t('computer.workbench.messages.idCopied'));
        } catch {
          message.error(t('computer.collection.copyIdFailed'));
        }
        break;
      case 'duplicate':
        onDuplicate();
        break;
      case 'delete':
        setDeleteOpen(true);
        break;
    }
  };

  return (
    <Card role="group" aria-label={nameWithId} className={styles.computerCard} classNames={{ body: styles.cardBody }}>
      <button type="button" onClick={onOpen} className={styles.identity} aria-label={nameWithId}
        aria-describedby={instance.description ? `${detailsHintId} ${descriptionId}` : detailsHintId}
        title={t('computer.openDetails')}>
        <span id={detailsHintId} hidden>{t('computer.openDetails')}</span>
        <ComputerDeviceIcon />
        <span className={styles.identityText}>
          <span className={styles.nameLine}>
            <span className={styles.name} title={instance.name}>{instance.name}</span>
            <RightOutlined aria-hidden="true" />
          </span>
          <span className={styles.descriptionLine}>
            <span className={styles.description} id={descriptionId} title={instance.description}>{instance.description}</span>
            {ambiguousName && <span className={styles.idSuffix} title={instance.id} aria-hidden="true">{instance.id.slice(-8)}</span>}
          </span>
        </span>
      </button>
      <div className={styles.status}>
        <Tag color={computerStatusColor[instance.status]}>
          {t(`computer.status.${instance.status}`)}
        </Tag>
        <Text type="secondary" title={connection.status === 'connected' && instance.connectionProfile
          ? t('computer.connectionProfile', { name: instance.connectionProfile }) : undefined}>
          {t(`computer.connection.${connection.status}`)}
        </Text>
        <div className={styles.adoption}>
          <ConfigAdoptionStatus instance={instance} compact onOpenRuntime={onOpen} />
        </div>
      </div>
      <div className={styles.employee}>
        <Text type="secondary" className={styles.employeeLabel}>{t('computer.collection.employee')}</Text>
        <Text ellipsis={{ tooltip: instance.robotName }} type={instance.robotName ? undefined : 'secondary'}>
          {instance.robotName || t('computer.noRobotBound')}
        </Text>
      </div>
      <div className={styles.actions}>
        <Tooltip title={startStopDisabledReason ?? startStopLabel}>
          <Button
            aria-label={startStopLabel}
            danger={stopAction}
            className={styles.runtimeAction}
            icon={startStopIcon}
            disabled={!canStartStop}
            loading={loading}
            onClick={onStartStop}
          />
        </Tooltip>
        {connection.showDisconnect ? (
          <Tooltip title={disconnectDisabledReason ?? t('connection.disconnect')}>
            <Button
              aria-label={t('connection.disconnect')}
              icon={<DisconnectOutlined />}
              disabled={!connection.actions.disconnect.enabled}
              loading={connection.isDisconnecting}
              onClick={onDisconnect}
            />
          </Tooltip>
        ) : (
          <Tooltip title={connectDisabledReason ?? t('connection.connect')}>
            <Button
              aria-label={t('connection.connect')}
              icon={<LinkOutlined />}
              disabled={!connection.canConnect}
              loading={connection.isConnecting}
              onClick={onConnect}
            />
          </Tooltip>
        )}
        <Dropdown trigger={['click']} open={active && menuOpen} onOpenChange={setMenuOpen} destroyOnHidden
          menu={{ id: menuId, 'aria-label': moreLabel, items: menuItems, onClick: handleMenuAction }} placement="bottomRight" autoFocus>
          <Button ref={menuTrigger} icon={<EllipsisOutlined />} aria-label={moreLabel}
            aria-haspopup="menu" aria-expanded={active && menuOpen} aria-controls={active && menuOpen ? menuId : undefined} />
        </Dropdown>
      </div>
      <Modal title={t('computer.confirmDelete')} open={deleteOpen} okText={t('computer.delete')}
        okButtonProps={{ danger: true }} confirmLoading={loading}
        onCancel={() => setDeleteOpen(false)} onOk={async () => { await onDelete(); setDeleteOpen(false); }}>
        <Text>{t('computer.workbench.deleteDescription', { name: instance.name })}</Text>
        {ambiguousName && <div><Text code>{instance.id}</Text></div>}
      </Modal>
    </Card>
  );
}

export function Computer({ targetMcpId, initialView = 'list', initialSection = 'top', navigationRevision = 0, onNavigate }: ComputerProps) {
  const action = usePageAction();
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    instances,
    loading,
    selectedInstanceId,
    fetchInstances,
    selectInstance,
    createInstance,
    updateInstance,
    duplicateInstance,
    deleteInstance,
    startInstance,
    stopInstance,
    restartInstance,
    connectSelectedTarget,
    disconnectConnection,
  } = useComputerStore();
  const { inspecting, inspectPackage } = usePortableConfigStore();
  const [view, setView] = useNavigationState<'list' | 'detail'>('computer.view', initialView);
  const [collectionLayout, setCollectionLayout] = useNavigationState<'list' | 'grid'>('computer.collectionLayout', 'list');
  const [search, setSearch] = useNavigationState('computer.search', '');
  const [modalMode, setModalMode] = useState<'create' | 'edit' | 'duplicate' | null>(null);
  const [targetInstance, setTargetInstance] = useState<ComputerInstance | null>(null);
  const [importPath, setImportPath] = useState<string | null>(null);
  const [form] = Form.useForm<{
    name: string;
    description?: string;
    copyRobotBinding?: boolean;
    skillHomeMode?: 'empty' | 'copy';
  }>();

  const computerDraft = useNavigationForm('computer.editor', form, { name: '' });

  useEffect(() => {
    fetchInstances();
  }, [fetchInstances]);

  useEffect(() => {
    setView(initialView);
  }, [initialView, navigationRevision, setView]);

  const selectedInstance = instances.find((instance) => instance.id === selectedInstanceId) ?? instances[0];
  const showDetail = view === 'detail' && selectedInstance;
  const query = search.trim().toLocaleLowerCase();
  const visibleInstances = instances.filter(({ name, description }) =>
    name.toLocaleLowerCase().includes(query) || description?.toLocaleLowerCase().includes(query),
  );
  const nameCounts = new Map<string, number>();
  for (const { name } of instances) nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1);
  const runningCount = instances.filter((instance) => usesStopAction(instance.status) && instance.status !== 'stopping').length;

  const openCreateModal = () => {
    setTargetInstance(null);
    setImportPath(null);
    setModalMode('create');
    form.setFieldsValue({ name: '', description: undefined });
  };

  const openEditModal = (instance: ComputerInstance) => {
    setTargetInstance(instance);
    setModalMode('edit');
    form.setFieldsValue({ name: instance.name, description: instance.description });
  };

  const openDuplicateModal = (instance: ComputerInstance) => {
    setTargetInstance(instance);
    setModalMode('duplicate');
    form.setFieldsValue({
      name: `${instance.name} Copy`,
      description: instance.description,
      copyRobotBinding: true,
      skillHomeMode: 'empty',
    });
  };

  const closeModal = () => {
    setModalMode(null);
    setTargetInstance(null);
    setImportPath(null);
    computerDraft.clearDraft();
    form.resetFields();
  };

  const chooseImportPackage = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        multiple: false,
      });
      if (!path) return;
      const inspection = await inspectPackage(path as string);
      setImportPath(path as string);
      form.setFieldsValue({
        name: inspection.originalName,
        description: inspection.description,
      });
    } catch (error) {
      message.error(String(error));
    }
  };

  const handleModalOk = async () => {
    const current = action();
    const values = await form.validateFields();
    if (!current()) return;
    try {
      if (modalMode === 'create') {
        await createInstance({
          name: values.name,
          description: values.description,
          importPath: importPath ?? undefined,
        });
        message.success(t('computer.messages.created'));
      } else if (modalMode === 'edit' && targetInstance) {
        await updateInstance(targetInstance.id, { name: values.name, description: values.description });
        message.success(t('computer.messages.updated'));
      } else if (modalMode === 'duplicate' && targetInstance) {
        await duplicateInstance({
          sourceId: targetInstance.id,
          name: values.name,
          description: values.description,
          copyRobotBinding: values.copyRobotBinding ?? true,
          skillHomeMode: values.skillHomeMode ?? 'empty',
        });
        message.success(t('computer.messages.duplicated'));
      }
      if (current()) closeModal();
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleDelete = async (instance: ComputerInstance) => {
    try {
      await deleteInstance(instance.id);
      message.success(t('computer.messages.deleted'));
      if (showDetail && selectedInstance.id === instance.id) {
        setView('list');
      }
    } catch (e) {
      message.error(String(e));
    }
  };

  const runRuntimeAction = async (
    instance: ComputerInstance,
    action: 'start' | 'restart',
  ) => {
    try {
      if (action === 'start') await startInstance(instance.id);
      if (action === 'restart') await restartInstance(instance.id);
      message.success(t(`computer.messages.${action === 'start' ? 'started' : 'restarted'}`));
    } catch (e) {
      if (isRuntimeInputCancelledError(e)) return;
      message.error(t('computer.messages.runtimeOperationFailed'));
    }
  };

  const handleStartStop = async (instance: ComputerInstance) => {
    if (!usesStopAction(instance.status)) {
      await runRuntimeAction(instance, 'start');
      return;
    }
    try {
      await stopInstance(instance.id);
      message.success(t('computer.messages.stopped'));
    } catch {
      message.error(t('computer.messages.runtimeOperationFailed'));
    }
  };

  const handleConnect = async (instance: ComputerInstance) => {
    try {
      await connectSelectedTarget(instance.id);
      message.success(t('connection.messages.connected'));
    } catch {
      message.error(t('computer.messages.connectionOperationFailed'));
    }
  };

  const handleDisconnect = async (instance: ComputerInstance) => {
    try {
      await disconnectConnection(instance.id);
      message.success(t('connection.messages.disconnected'));
    } catch {
      message.error(t('computer.messages.connectionOperationFailed'));
    }
  };

  useEffect(() => {
    if (targetInstance && !instances.some((instance) => instance.id === targetInstance.id)) {
      setModalMode(null);
      setTargetInstance(null);
      form.resetFields();
    }
  }, [form, instances, targetInstance]);

  const modalTitle = modalMode === 'create'
    ? t('computer.create')
    : modalMode === 'edit'
      ? t('computer.collection.editIdentity')
      : t('computer.duplicate');

  const renderComputerModal = () => (
    <Modal
      title={modalTitle}
      open={modalMode !== null}
      onCancel={closeModal}
      onOk={() => handleModalOk()}
      confirmLoading={loading}
      forceRender
      destroyOnHidden
    >
      <Form name="computer-editor" form={form} onValuesChange={(_, values) => computerDraft.onValuesChange(values)} layout="vertical">
        <Form.Item
          name="name"
          label={t('computer.form.name')}
          rules={[
            {
              validator: (_, value) => value?.trim()
                ? Promise.resolve()
                : Promise.reject(new Error(t('computer.form.nameRequired'))),
            },
          ]}
        >
          <Input autoFocus />
        </Form.Item>
        <Form.Item name="description" label={t('computer.form.description')}>
          <Input.TextArea rows={3} />
        </Form.Item>
        {modalMode === 'create' && (
          <Form.Item label={t('computer.portable.importLabel')}>
            <Space direction="vertical" size={4}>
              <Button loading={inspecting} onClick={() => void chooseImportPackage()}>
                {t('computer.portable.chooseFile')}
              </Button>
              {importPath && (
                <Text type="secondary">
                  {t('computer.portable.importSelected', {
                    name: importPath.split(/[\\/]/).pop(),
                  })}
                </Text>
              )}
            </Space>
          </Form.Item>
        )}
        {modalMode === 'duplicate' && (
          <>
            <Form.Item name="copyRobotBinding" label={t('computer.form.copyRobotBinding')} valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="skillHomeMode" label={t('computer.form.skillHomeMode')}>
              <Select
                options={[
                  { value: 'empty', label: t('computer.form.skillHomeEmpty') },
                  { value: 'copy', label: t('computer.form.skillHomeCopy') },
                ]}
              />
            </Form.Item>
          </>
        )}
      </Form>
    </Modal>
  );

  if (loading && instances.length === 0) {
    return <Skeleton active paragraph={{ rows: 4 }} />;
  }

  return (
    <div>
      {selectedInstance && (
        <NavigationScope key={selectedInstance.id} id={`computer:${selectedInstance.id}`}>
          <PageHost name="computer-detail" active={Boolean(showDetail)}>
            <ComputerWorkbench
              instance={selectedInstance}
              loading={loading}
              targetMcpId={targetMcpId}
              initialSection={initialSection}
              navigationRevision={navigationRevision}
              onBack={() => { setView('list'); onNavigate?.('computer'); }}
              onNavigate={onNavigate}
              onOpenSettings={() => onNavigate?.('computer-settings:general')}
              onDelete={() => handleDelete(selectedInstance)}
              onStartStop={() => { void handleStartStop(selectedInstance); }}
              onRestart={() => { void runRuntimeAction(selectedInstance, 'restart'); }}
              onConnect={() => { void handleConnect(selectedInstance); }}
              onDisconnect={() => { void handleDisconnect(selectedInstance); }}
              onOpenPlugin={(owner) => onNavigate?.(computerSettingsNavigationKey('plugins', owner))}
              onOpenPermissionHelp={() => onNavigate?.(permissionHelpRoute('mcp'))}
            />
          </PageHost>
        </NavigationScope>
      )}
      <PageHost name="computer-list" active={!showDetail}>
        <div className={styles.collectionArea}>
          <div className={styles.listHeader}>
            <div>
              <Title level={3} className={styles.listTitle}>{t('computer.title')}</Title>
              <Text type="secondary">{t('computer.collection.count', { count: instances.length })} · {t('computer.collection.running', { count: runningCount })}</Text>
            </div>
            <Button type="primary" onClick={openCreateModal}>{t('computer.create')}</Button>
          </div>
          <div className={styles.toolbar}>
            <Input type="search" className={styles.search} value={search} onChange={(event) => setSearch(event.target.value)}
              prefix={<SearchOutlined />} placeholder={t('computer.collection.search')} aria-label={t('computer.collection.search')} />
            <Segmented
              aria-label={t('computer.views.label')}
              value={collectionLayout}
              onChange={(value) => setCollectionLayout(value as 'list' | 'grid')}
              options={[
                { value: 'list', label: t('computer.views.list'), icon: <BarsOutlined /> },
                { value: 'grid', label: t('computer.views.grid'), icon: <AppstoreOutlined /> },
              ]}
            />
          </div>
          {visibleInstances.length === 0 ? (
            <Empty description={t(instances.length === 0 ? 'computer.empty' : 'computer.collection.noMatches')} />
          ) : (
            <div className={collectionLayout === 'grid' ? styles.collectionGrid : styles.collectionList}>
              {collectionLayout === 'list' && <div className={styles.columnHeaders} aria-hidden="true">
                <span>{t('computer.title')}</span>
                <span>{t('computer.collection.status')}</span>
                <span>{t('computer.collection.employee')}</span>
                <span>{t('computer.collection.actions')}</span>
              </div>}
              {visibleInstances.map((instance) => (
                <ComputerCard
                  key={instance.id}
                  instance={instance}
                  ambiguousName={(nameCounts.get(instance.name) ?? 0) > 1}
                  onOpen={() => {
                    selectInstance(instance.id);
                    setView('detail');
                    onNavigate?.('computer-detail:top');
                  }}
                  onOpenSettings={onNavigate && (() => {
                    selectInstance(instance.id);
                    onNavigate(computerSettingsNavigationKey('general'));
                  })}
                  onEdit={() => openEditModal(instance)}
                  onDuplicate={() => openDuplicateModal(instance)}
                  onStartStop={() => handleStartStop(instance)}
                  onConnect={() => handleConnect(instance)}
                  onDisconnect={() => handleDisconnect(instance)}
                  onDelete={() => handleDelete(instance)}
                  loading={loading}
                />
              ))}
            </div>
          )}
        </div>
        {renderComputerModal()}
      </PageHost>
    </div>
  );
}
