import { usePageAction } from '@/components/Navigation/pageActivityState';
import { PageHost } from '@/components/Navigation/PageHost';
import { NavigationScope } from '@/components/Navigation/NavigationMemory';
import { useNavigationForm, useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { PageModal as Modal, PagePopconfirm as Popconfirm } from '@/components/Navigation/PageOverlays';
import { App, Button, Card, Empty, Form, Input, Select, Segmented, Skeleton, Space, Switch, Tag, Tooltip, Typography } from 'antd';
import {
  ApiOutlined,
  AppstoreOutlined,
  BarsOutlined,
  CopyOutlined,
  DesktopOutlined,
  DeleteOutlined,
  DisconnectOutlined,
  EditOutlined,
  LinkOutlined,
  PlayCircleOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
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
import styles from './Computer.module.css';
import {
  computerStatusColor,
  connectDisabledReasonTranslationKey,
  resolveComputerConnection,
  usesStopAction,
} from './computerActions';

const { Title, Text } = Typography;

interface ComputerProps {
  navigationRevision?: number;
  initialView?: 'list' | 'detail';
  initialSection?: ComputerWorkbenchSection;
  onNavigate?: (key: string) => void;
}

function ComputerCard({
  instance,
  onOpen,
  onEdit,
  onDuplicate,
  onStart,
  onStop,
  onConnect,
  onDisconnect,
  onDelete,
  loading,
}: {
  instance: ComputerInstance;
  onOpen: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onStart: () => void;
  onStop: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onDelete: () => void;
  loading: boolean;
}) {
  const { t } = useTranslation();
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
  const startStopAction = stopAction ? onStop : onStart;
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

  return (
    <Card className={styles.computerCard} classNames={{ body: styles.cardBody }}>
      <div className={styles.identity}>
        <div className={styles.deviceIcon} aria-hidden="true"><DesktopOutlined /></div>
        <div className={styles.identityText}>
          <Button type="text" onClick={onOpen} className={styles.name}>{instance.name}</Button>
          {instance.description && <Text className={styles.description}>{instance.description}</Text>}
          <Text type="secondary" copyable className={styles.identifier}>{instance.id}</Text>
        </div>
      </div>
      <div className={styles.metadata}>
        <Tag color={computerStatusColor[instance.status]}>
          {t(`computer.status.${instance.status}`)}
        </Tag>
        <Space wrap size={[4, 6]}>
          <Tag color={connection.status === 'connected' ? 'green' : 'default'}>
            {t(`computer.connection.${connection.status}`)}
          </Tag>
          <Tag icon={<ApiOutlined />}>
            {t('computer.mcpServers', { count: instance.mcpServerCount })}
          </Tag>
        </Space>
        <Text type="secondary">
          {instance.robotName
            ? t('computer.boundRobot', { name: instance.robotName })
            : t('computer.noRobotBound')}
        </Text>
        {instance.connectionStatus === 'connected' && instance.connectionProfile && (
          <Text type="secondary">
            {t('computer.connectionProfile', { name: instance.connectionProfile })}
          </Text>
        )}
      </div>
      <div className={styles.actions}>
        <Tooltip title={startStopDisabledReason ?? startStopLabel}>
          <Button
            aria-label={startStopLabel}
            type="text"
            danger={stopAction}
            className={styles.runtimeAction}
            icon={startStopIcon}
            disabled={!canStartStop}
            loading={loading}
            onClick={startStopAction}
          >{startStopLabel}</Button>
        </Tooltip>
        {connection.showDisconnect ? (
          <Tooltip title={disconnectDisabledReason ?? t('connection.disconnect')}>
            <Button
              aria-label={t('connection.disconnect')}
              type="text"
              danger
              icon={<DisconnectOutlined />}
              disabled={!connection.actions.disconnect.enabled}
              loading={connection.isDisconnecting}
              onClick={onDisconnect}
            >{t('connection.disconnect')}</Button>
          </Tooltip>
        ) : (
          <Tooltip title={connectDisabledReason ?? t('connection.connect')}>
            <Button
              aria-label={t('connection.connect')}
              type="text"
              icon={<LinkOutlined />}
              disabled={!connection.canConnect}
              loading={connection.isConnecting}
              onClick={onConnect}
            >{t('connection.connect')}</Button>
          </Tooltip>
        )}
        <Button aria-label={t('computer.edit')} type="text" icon={<EditOutlined />} onClick={onEdit}>
          {t('computer.edit')}
        </Button>
        <Button aria-label={t('computer.duplicate')} type="text" icon={<CopyOutlined />} onClick={onDuplicate}>
          {t('computer.duplicate')}
        </Button>
        <Popconfirm title={t('computer.confirmDelete')} onConfirm={onDelete}>
          <Button aria-label={t('computer.delete')} type="text" danger icon={<DeleteOutlined />} loading={loading}>
            {t('computer.delete')}
          </Button>
        </Popconfirm>
      </div>
    </Card>
  );
}

export function Computer({ initialView = 'list', initialSection = 'top', navigationRevision = 0, onNavigate }: ComputerProps) {
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
      ? t('computer.edit')
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
              initialSection={initialSection}
              navigationRevision={navigationRevision}
              onBack={() => { setView('list'); onNavigate?.('computer'); }}
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
        <div className={styles.listHeader}>
          <Title level={4} style={{ margin: 0 }}>{t('computer.title')}</Title>
          <Space wrap>
            <Segmented
              aria-label={t('computer.views.label')}
              value={collectionLayout}
              onChange={(value) => setCollectionLayout(value as 'list' | 'grid')}
              options={[
                { value: 'list', label: t('computer.views.list'), icon: <BarsOutlined /> },
                { value: 'grid', label: t('computer.views.grid'), icon: <AppstoreOutlined /> },
              ]}
            />
            <Button type="primary" onClick={openCreateModal}>{t('computer.create')}</Button>
          </Space>
        </div>

        {instances.length === 0 ? (
          <Empty description={t('computer.empty')} />
        ) : (
          <div className={collectionLayout === 'grid' ? styles.collectionGrid : styles.collectionList}>
            {instances.map((instance) => (
              <ComputerCard
                key={instance.id}
                instance={instance}
                onOpen={() => {
                  selectInstance(instance.id);
                  setView('detail');
                  onNavigate?.('computer-detail:top');
                }}
                onEdit={() => openEditModal(instance)}
                onDuplicate={() => openDuplicateModal(instance)}
                onStart={() => handleStartStop(instance)}
                onStop={() => handleStartStop(instance)}
                onConnect={() => handleConnect(instance)}
                onDisconnect={() => handleDisconnect(instance)}
                onDelete={() => handleDelete(instance)}
                loading={loading}
              />
            ))}
          </div>
        )}
        {renderComputerModal()}
      </PageHost>
    </div>
  );
}
