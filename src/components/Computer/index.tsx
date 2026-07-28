import { App, Button, Card, Col, Empty, Form, Input, Modal, Popconfirm, Row, Select, Skeleton, Space, Switch, Tabs, Tag, Tooltip, Typography } from 'antd';
import {
  ApiOutlined,
  AppstoreOutlined,
  BugOutlined,
  CloudServerOutlined,
  CopyOutlined,
  DesktopOutlined,
  DeleteOutlined,
  DisconnectOutlined,
  EditOutlined,
  FileTextOutlined,
  FormOutlined,
  LinkOutlined,
  ReadOutlined,
  PlayCircleOutlined,
  SettingOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
import {
  useComputerStore,
  type ComputerConnectionTarget,
  type ComputerInstance,
  type ComputerStatus,
} from '@/stores/computerStore';
import {
  formatRuntimeActionError,
  isMissingRuntimeInputError,
  type MissingRuntimeInputError,
} from '@/utils/runtimeActionError';
import { McpConfig } from '@/components/McpConfig';
import { InputVariables } from '@/components/InputVariables';
import { RuntimeInputPrompt } from '@/components/InputVariables/RuntimeInputPrompt';
import { DesktopResources } from '@/components/DesktopResources';
import { DebugPanel } from '@/components/DebugPanel';
import { LogViewer } from '@/components/LogViewer';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';
import { toComputerDetailTab, type ComputerDetailTab } from './tabs';
import { ComputerOverview } from './ComputerOverview';
import { ComputerRuntimeSettings } from './ComputerRuntimeSettings';
import { ComputerRuntime } from './ComputerRuntime';
import { MarketplaceTab } from './MarketplaceTab';
import { SkillsTab } from './SkillsTab';

const { Title, Text } = Typography;

const statusColor: Record<ComputerStatus, string> = {
  running: 'success',
  not_running: 'default',
  starting: 'processing',
  stopping: 'processing',
  degraded: 'warning',
  error: 'error',
};

function usesStopAction(status: ComputerStatus): boolean {
  return status === 'running' || status === 'degraded' || status === 'stopping';
}

function isConnectionTargetConnectable(target?: ComputerConnectionTarget | null): boolean {
  if (!target) return false;
  if (target.type === 'manager_robot') return target.robotAccountId != null;
  return true;
}

interface ComputerProps {
  initialView?: 'list' | 'detail';
  initialTab?: ComputerDetailTab;
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
  const connectionTarget = instance.connectionPolicy.target;
  const connectionTargetSelected = Boolean(connectionTarget);
  const connectionTargetConnectable = isConnectionTargetConnectable(connectionTarget);
  const connectionActions = instance.connectionState?.actions ?? {
    connect: instance.runtime.actions.connect,
    disconnect: instance.runtime.actions.disconnect,
  };
  const connectionStatus = instance.connectionState?.status
    ?? (instance.clientConnectionPresent ? 'connected' : instance.connectionStatus);
  const canConnect = connectionActions.connect.enabled && connectionTargetConnectable;
  const showDisconnect = connectionActions.disconnect.enabled
    || connectionStatus === 'disconnecting';
  const disconnectDisabledReason = connectionActions.disconnect.disabled_reason
    ? t(`computer.connectionActions.disabledReasons.${connectionActions.disconnect.disabled_reason}`)
    : undefined;
  const connectDisabledReason = !connectionActions.connect.enabled
    ? t(`computer.connectionActions.disabledReasons.${connectionActions.connect.disabled_reason}`)
    : !connectionTargetSelected
      ? t('computer.connectionActions.requiresTarget')
      : !connectionTargetConnectable
        ? t('computer.connectionActions.missingRobotAccountId')
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
        <Tag color={statusColor[instance.status]}>
          {t(`computer.status.${instance.status}`)}
        </Tag>
      }
    >
      <div style={{ display: 'flex', gap: 16, alignItems: 'stretch' }}>
        <Space direction="vertical" size={8} style={{ flex: 1, minWidth: 0 }}>
          <Text type="secondary" copyable>{instance.id}</Text>
          {instance.description && <Text>{instance.description}</Text>}
          <Space wrap>
            <Tag color={connectionStatus === 'connected' ? 'green' : 'default'}>
              {t(`computer.connection.${connectionStatus}`)}
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
          {showDisconnect ? (
            <Tooltip title={disconnectDisabledReason ?? t('connection.disconnect')} placement="right">
              <Button
                aria-label={t('connection.disconnect')}
                type="text"
                danger
                icon={<DisconnectOutlined />}
                disabled={!connectionActions.disconnect.enabled}
                loading={connectionStatus === 'disconnecting'}
                onClick={onDisconnect}
              />
            </Tooltip>
          ) : (
            <Tooltip title={connectDisabledReason ?? t('connection.connect')} placement="right">
              <Button
                aria-label={t('connection.connect')}
                type="text"
                icon={<LinkOutlined />}
                disabled={!canConnect}
                loading={connectionStatus === 'connecting'}
                style={{ color: canConnect ? '#1677ff' : undefined }}
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

export function Computer({ initialView = 'list', initialTab = 'overview', onNavigate }: ComputerProps) {
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
  const { manualTargets, fetchManualTargets } = useConnectionTargetStore();
  const [view, setView] = useState<'list' | 'detail'>(initialView);
  const [activeTab, setActiveTab] = useState<ComputerDetailTab>(initialTab);
  const [modalMode, setModalMode] = useState<'create' | 'edit' | 'duplicate' | null>(null);
  const [targetInstance, setTargetInstance] = useState<ComputerInstance | null>(null);
  const [runtimeInputPrompt, setRuntimeInputPrompt] = useState<{
    instanceId: string;
    error: MissingRuntimeInputError;
    action: 'start' | 'restart';
  } | null>(null);
  const [form] = Form.useForm<{
    name: string;
    description?: string;
    copyRobotBinding?: boolean;
    connectionTargetId?: string;
    skillHomeMode?: 'empty' | 'copy';
  }>();

  useEffect(() => {
    fetchInstances();
  }, [fetchInstances]);

  useEffect(() => {
    setView(initialView);
    setActiveTab(initialTab);
  }, [initialView, initialTab]);

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
    fetchManualTargets();
    form.setFieldsValue({
      name: `${instance.name} Copy`,
      description: instance.description,
      copyRobotBinding: true,
      connectionTargetId: undefined,
      skillHomeMode: 'empty',
    });
  };

  const closeModal = () => {
    setModalMode(null);
    setTargetInstance(null);
    form.resetFields();
  };

  const handleModalOk = async () => {
    const values = await form.validateFields();
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
          connectionTargetId: values.connectionTargetId,
          skillHomeMode: values.skillHomeMode ?? 'empty',
        });
        message.success(t('computer.messages.duplicated'));
      }
      closeModal();
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
      setRuntimeInputPrompt(null);
      message.success(t(`computer.messages.${action === 'start' ? 'started' : 'restarted'}`));
    } catch (e) {
      if (isMissingRuntimeInputError(e)) {
        setRuntimeInputPrompt({ instanceId: instance.id, error: e, action });
        return;
      }
      message.error(formatRuntimeActionError(e));
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
    } catch (e) {
      message.error(formatRuntimeActionError(e));
    }
  };

  const handleConnect = async (instance: ComputerInstance) => {
    try {
      await connectSelectedTarget(instance.id);
      message.success(t('connection.messages.connected'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleDisconnect = async (instance: ComputerInstance) => {
    try {
      await disconnectConnection(instance.id);
      message.success(t('connection.messages.disconnected'));
    } catch (e) {
      message.error(String(e));
    }
  };

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
      <Form form={form} layout="vertical">
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
            <Form.Item name="connectionTargetId" label={t('computer.form.connectionTarget')}>
              <Select
                allowClear
                placeholder={t('computer.form.useOriginalConnectionConfig')}
                options={manualTargets.map((target) => ({
                  value: target.id,
                  label: `${target.name} (${target.office_id})`,
                }))}
              />
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

  const renderRuntimeInputPrompt = () => runtimeInputPrompt && (
    <RuntimeInputPrompt
      key={`${runtimeInputPrompt.instanceId}:${runtimeInputPrompt.error.input_id}`}
      instanceId={runtimeInputPrompt.instanceId}
      error={runtimeInputPrompt.error}
      onCancel={() => setRuntimeInputPrompt(null)}
      onSubmitted={async () => {
        const instance = instances.find((item) => item.id === runtimeInputPrompt.instanceId);
        if (instance) await runRuntimeAction(instance, runtimeInputPrompt.action);
      }}
    />
  );

  if (loading && instances.length === 0) {
    return <Skeleton active paragraph={{ rows: 4 }} />;
  }

  const selectedConnectionTarget = selectedInstance?.connectionPolicy.target;
  const selectedConnectionState = selectedInstance?.connectionState;
  const selectedConnectionActions = selectedConnectionState?.actions ?? (selectedInstance ? {
    connect: selectedInstance.runtime.actions.connect,
    disconnect: selectedInstance.runtime.actions.disconnect,
  } : undefined);
  const selectedConnectionStatus = selectedConnectionState?.status
    ?? (selectedInstance?.clientConnectionPresent
      ? 'connected'
      : selectedInstance?.connectionStatus);
  const selectedShowDisconnect = Boolean(selectedConnectionActions?.disconnect.enabled)
    || selectedConnectionStatus === 'disconnecting';
  const selectedConnectionTargetSelected = Boolean(selectedConnectionTarget);
  const selectedConnectionTargetConnectable = isConnectionTargetConnectable(selectedConnectionTarget);
  const selectedCanConnect = Boolean(selectedConnectionActions?.connect.enabled)
    && selectedConnectionTargetConnectable;
  const selectedStopAction = selectedInstance ? usesStopAction(selectedInstance.status) : false;
  const selectedStartStopCapability = selectedInstance
    ? selectedStopAction
      ? selectedInstance.runtime.actions.stop
      : selectedInstance.runtime.actions.start
    : undefined;
  const selectedStartStopDisabledReason = selectedStartStopCapability?.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${selectedStartStopCapability.disabled_reason}`)
    : undefined;
  const selectedDisconnectDisabledReason = selectedConnectionActions?.disconnect.disabled_reason
    ? t(`computer.connectionActions.disabledReasons.${selectedConnectionActions.disconnect.disabled_reason}`)
    : undefined;
  const selectedConnectDisabledReason = !selectedConnectionActions?.connect.enabled
    ? t(`computer.connectionActions.disabledReasons.${selectedConnectionActions?.connect.disabled_reason}`)
    : !selectedConnectionTargetSelected
      ? t('computer.connectionActions.requiresTarget')
      : !selectedConnectionTargetConnectable
        ? t('computer.connectionActions.missingRobotAccountId')
        : undefined;

  if (showDetail) {
    return (
      <div>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', gap: 16, alignItems: 'flex-start' }}>
            <div>
              <Title level={4} style={{ margin: 0 }}>{selectedInstance.name}</Title>
              <Text type="secondary" copyable>{selectedInstance.id}</Text>
              {selectedInstance.description && (
                <Text style={{ display: 'block', marginTop: 8 }}>{selectedInstance.description}</Text>
              )}
              <Space wrap style={{ marginTop: 8 }}>
                <Tag color={statusColor[selectedInstance.status]}>
                  {t(`computer.status.${selectedInstance.status}`)}
                </Tag>
                <Tag color={selectedConnectionStatus === 'connected' ? 'green' : 'default'}>
                  {t(`computer.connection.${selectedConnectionStatus}`)}
                </Tag>
                <Text type="secondary">
                  {selectedInstance.robotName
                    ? t('computer.boundRobot', { name: selectedInstance.robotName })
                    : t('computer.noRobotBound')}
                </Text>
                {selectedInstance.connectionStatus === 'connected' && selectedInstance.connectionProfile && (
                  <Text type="secondary">
                    {t('computer.connectionProfile', { name: selectedInstance.connectionProfile })}
                  </Text>
                )}
              </Space>
            </div>
            <Space wrap align="start">
              <Button onClick={() => setView('list')}>
                {t('computer.backToList')}
              </Button>
              <Space direction="vertical" size={2}>
                <Text type="secondary">{t('computer.actionGroups.profile')}</Text>
                <Space.Compact>
                  <Button icon={<EditOutlined />} onClick={() => openEditModal(selectedInstance)}>
                    {t('computer.edit')}
                  </Button>
                  <Button icon={<CopyOutlined />} onClick={() => openDuplicateModal(selectedInstance)}>
                    {t('computer.duplicate')}
                  </Button>
                  <Popconfirm title={t('computer.confirmDelete')} onConfirm={() => handleDelete(selectedInstance)}>
                    <Button danger icon={<DeleteOutlined />} loading={loading}>
                      {t('computer.delete')}
                    </Button>
                  </Popconfirm>
                </Space.Compact>
              </Space>
              <Space direction="vertical" size={2}>
                <Text type="secondary">{t('computer.actionGroups.runtime')}</Text>
                <Space.Compact>
                  {selectedShowDisconnect ? (
                    <Tooltip title={selectedDisconnectDisabledReason}>
                      <Button
                        danger
                        icon={<DisconnectOutlined />}
                        disabled={!selectedConnectionActions?.disconnect.enabled}
                        loading={selectedConnectionStatus === 'disconnecting'}
                        onClick={() => handleDisconnect(selectedInstance)}
                      >
                        {t('connection.disconnect')}
                      </Button>
                    </Tooltip>
                  ) : (
                    <Tooltip title={selectedConnectDisabledReason}>
                      <Button
                        type="primary"
                        icon={<LinkOutlined />}
                        disabled={!selectedCanConnect}
                        loading={selectedConnectionStatus === 'connecting'}
                        onClick={() => handleConnect(selectedInstance)}
                      >
                        {t('connection.connect')}
                      </Button>
                    </Tooltip>
                  )}
                  <Tooltip title={selectedStartStopDisabledReason}>
                    <Button
                      icon={selectedStopAction ? <StopOutlined /> : <PlayCircleOutlined />}
                      disabled={!selectedStartStopCapability?.enabled}
                      loading={loading}
                      onClick={() => handleStartStop(selectedInstance)}
                    >
                      {selectedInstance.status === 'starting'
                        ? t('computer.runtime.actionProgress.starting')
                        : selectedInstance.status === 'stopping'
                          ? t('computer.runtime.actionProgress.stopping')
                          : selectedStopAction
                            ? t('computer.stop')
                            : t('computer.start')}
                    </Button>
                  </Tooltip>
                </Space.Compact>
              </Space>
            </Space>
          </div>

          <Tabs
            activeKey={activeTab}
            onChange={(key) => setActiveTab(toComputerDetailTab(key))}
            items={[
              { key: 'overview', label: <><DesktopOutlined /> {t('dashboard.overview')}</>, children: <ComputerOverview instanceId={selectedInstance.id} onOpenTab={setActiveTab} /> },
              { key: 'mcp', label: <><ApiOutlined /> {t('mcp.servers')}</>, children: <McpConfig instanceId={selectedInstance.id} /> },
              { key: 'skills', label: <><ReadOutlined /> {t('skills.title')}</>, children: <SkillsTab instanceId={selectedInstance.id} onOpenMcpTab={() => setActiveTab('mcp')} /> },
              { key: 'marketplace', label: <><AppstoreOutlined /> {t('marketplace.title')}</>, children: <MarketplaceTab instanceId={selectedInstance.id} /> },
              { key: 'inputs', label: <><FormOutlined /> {t('inputs.title')}</>, children: <InputVariables instanceId={selectedInstance.id} /> },
              { key: 'connection', label: <><CloudServerOutlined /> {t('computer.robotConnection')}</>, children: <RobotConnectionPanel instanceId={selectedInstance.id} onNavigate={onNavigate} /> },
              { key: 'resources', label: <><DesktopOutlined /> {t('resources.title')}</>, children: <DesktopResources instanceId={selectedInstance.id} /> },
              { key: 'debug', label: <><BugOutlined /> {t('nav.debugPanel')}</>, children: <DebugPanel instanceId={selectedInstance.id} /> },
              { key: 'logs', label: <><FileTextOutlined /> {t('logs.title')}</>, children: <LogViewer instanceId={selectedInstance.id} /> },
              { key: 'configuration', label: <><SettingOutlined /> {t('common.configuration')}</>, children: <ComputerRuntimeSettings instance={selectedInstance} /> },
              {
                key: 'runtime',
                label: <><PlayCircleOutlined /> {t('computer.runtime.title')}</>,
                children: (
                  <ComputerRuntime
                    instance={selectedInstance}
                    loading={loading}
                    canConnect={selectedCanConnect}
                    connectDisabledReason={selectedConnectDisabledReason}
                    onStartStop={() => { void handleStartStop(selectedInstance); }}
                    onRestart={() => { void runRuntimeAction(selectedInstance, 'restart'); }}
                    onConnect={() => { void handleConnect(selectedInstance); }}
                    onDisconnect={() => { void handleDisconnect(selectedInstance); }}
                  />
                ),
              },
            ]}
          />
        </Space>
        {renderComputerModal()}
        {renderRuntimeInputPrompt()}
      </div>
    );
  }

  return (
    <div>
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
      {renderRuntimeInputPrompt()}
    </div>
  );
}
