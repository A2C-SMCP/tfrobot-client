import { usePageAction } from '@/components/Navigation/pageActivityState';
import { PageHost } from '@/components/Navigation/PageHost';
import { NavigationScope } from '@/components/Navigation/NavigationMemory';
import { useNavigationForm, useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { PageModal as Modal, PagePopconfirm as Popconfirm } from '@/components/Navigation/PageOverlays';
import { App, Button, Card, Col, Empty, Form, Input, Row, Select, Skeleton, Space, Switch, Tag, Tooltip, Typography } from 'antd';
import {
  ApiOutlined,
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
import { isRuntimeInputCancelledError } from '@/utils/runtimeActionError';
import {
  computerSettingsNavigationKey,
  type ComputerWorkbenchSection,
} from './tabs';
import { ComputerWorkbench } from './ComputerWorkbench';
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
  const startStopColor = stopAction ? '#fa8c16' : '#52c41a';
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
    <Card
      title={
        <Space>
          <DesktopOutlined />
          <Button
            type="link"
            onClick={onOpen}
            style={{ padding: 0, height: 'auto', fontWeight: 600 }}
          >
            {instance.name}
          </Button>
        </Space>
      }
      extra={
        <Tag color={computerStatusColor[instance.status]}>
          {t(`computer.status.${instance.status}`)}
        </Tag>
      }
    >
      <div style={{ display: 'flex', gap: 16, alignItems: 'stretch' }}>
        <Space direction="vertical" size={8} style={{ flex: 1, minWidth: 0 }}>
          <Text type="secondary" copyable>{instance.id}</Text>
          {instance.description && <Text>{instance.description}</Text>}
          <Space wrap>
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
        </Space>

        <Space
          direction="vertical"
          size={6}
          style={{ borderLeft: '1px solid #f0f0f0', paddingLeft: 12, justifyContent: 'center' }}
        >
          <Tooltip title={startStopDisabledReason ?? startStopLabel} placement="right">
            <Button
              aria-label={startStopLabel}
              type="text"
              icon={startStopIcon}
              disabled={!canStartStop}
              loading={loading}
              style={{ color: startStopColor }}
              onClick={startStopAction}
            />
          </Tooltip>
          {connection.showDisconnect ? (
            <Tooltip title={disconnectDisabledReason ?? t('connection.disconnect')} placement="right">
              <Button
                aria-label={t('connection.disconnect')}
                type="text"
                danger
                icon={<DisconnectOutlined />}
                disabled={!connection.actions.disconnect.enabled}
                loading={connection.isDisconnecting}
                onClick={onDisconnect}
              />
            </Tooltip>
          ) : (
            <Tooltip title={connectDisabledReason ?? t('connection.connect')} placement="right">
              <Button
                aria-label={t('connection.connect')}
                type="text"
                icon={<LinkOutlined />}
                disabled={!connection.canConnect}
                loading={connection.isConnecting}
                style={{ color: connection.canConnect ? '#1677ff' : undefined }}
                onClick={onConnect}
              />
            </Tooltip>
          )}
          <Tooltip title={t('computer.edit')} placement="right">
            <Button aria-label={t('computer.edit')} type="text" icon={<EditOutlined />} onClick={onEdit} />
          </Tooltip>
          <Tooltip title={t('computer.duplicate')} placement="right">
            <Button aria-label={t('computer.duplicate')} type="text" icon={<CopyOutlined />} onClick={onDuplicate} />
          </Tooltip>
          <Tooltip title={t('computer.delete')} placement="right">
            <Popconfirm title={t('computer.confirmDelete')} onConfirm={onDelete}>
              <Button aria-label={t('computer.delete')} type="text" danger icon={<DeleteOutlined />} loading={loading} />
            </Popconfirm>
          </Tooltip>
        </Space>
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
  const [view, setView] = useNavigationState<'list' | 'detail'>('computer.view', initialView);
  const [modalMode, setModalMode] = useState<'create' | 'edit' | 'duplicate' | null>(null);
  const [targetInstance, setTargetInstance] = useState<ComputerInstance | null>(null);
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
    computerDraft.clearDraft();
    form.resetFields();
  };

  const handleModalOk = async () => {
    const current = action();
    const values = await form.validateFields();
    if (!current()) return;
    try {
      if (modalMode === 'create') {
        await createInstance({ name: values.name, description: values.description });
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
            />
          </PageHost>
        </NavigationScope>
      )}
      <PageHost name="computer-list" active={!showDetail}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('computer.title')}</Title>
        <Button type="primary" onClick={openCreateModal}>
          {t('computer.create')}
        </Button>
      </div>

      {instances.length === 0 ? (
        <Empty description={t('computer.empty')} />
      ) : (
        <Row gutter={[16, 16]}>
          {instances.map((instance) => (
            <Col key={instance.id} xs={24} lg={12} xl={8}>
              <ComputerCard
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
            </Col>
          ))}
        </Row>
      )}
      {renderComputerModal()}
      </PageHost>
    </div>
  );
}
