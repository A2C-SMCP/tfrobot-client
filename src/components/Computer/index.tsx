import { App, Button, Card, Col, Empty, Form, Input, Modal, Popconfirm, Row, Select, Skeleton, Space, Switch, Tabs, Tag, Tooltip, Typography } from 'antd';
import {
  ApiOutlined,
  BugOutlined,
  CloudServerOutlined,
  CopyOutlined,
  DesktopOutlined,
  DeleteOutlined,
  EditOutlined,
  FileTextOutlined,
  FormOutlined,
  PlayCircleOutlined,
  SettingOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
import { useComputerStore, type ComputerInstance, type ComputerStatus } from '@/stores/computerStore';
import { McpConfig } from '@/components/McpConfig';
import { InputVariables } from '@/components/InputVariables';
import { DesktopResources } from '@/components/DesktopResources';
import { DebugPanel } from '@/components/DebugPanel';
import { LogViewer } from '@/components/LogViewer';
import { RuntimeSettings } from '@/components/Settings/RuntimeSettings';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import { useConnectionTargetStore } from '@/stores/connectionTargetStore';
import { toComputerDetailTab, type ComputerDetailTab } from './tabs';
import { ComputerOverview } from './ComputerOverview';

const { Title, Text } = Typography;

const statusColor: Record<ComputerStatus, string> = {
  running: 'success',
  stopped: 'default',
  error: 'error',
};

interface ComputerProps {
  initialView?: 'list' | 'detail';
  initialTab?: ComputerDetailTab;
}

function ComputerCard({
  instance,
  onOpen,
  onEdit,
  onDuplicate,
  onStart,
  onStop,
  onDelete,
  loading,
}: {
  instance: ComputerInstance;
  onOpen: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onStart: () => void;
  onStop: () => void;
  onDelete: () => void;
  loading: boolean;
}) {
  const { t } = useTranslation();
  const startStopLabel = instance.status === 'running' ? t('computer.stop') : t('computer.start');
  const startStopIcon = instance.status === 'running' ? <StopOutlined /> : <PlayCircleOutlined />;
  const startStopColor = instance.status === 'running' ? '#fa8c16' : '#52c41a';
  const startStopAction = instance.status === 'running' ? onStop : onStart;

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
            <Tag color={instance.connectionStatus === 'connected' ? 'green' : 'default'}>
              {t(`computer.connection.${instance.connectionStatus}`)}
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
          {instance.connectionProfile && (
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
          <Tooltip title={startStopLabel} placement="right">
            <Button
              aria-label={startStopLabel}
              type="text"
              icon={startStopIcon}
              loading={loading}
              style={{ color: startStopColor }}
              onClick={startStopAction}
            />
          </Tooltip>
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

export function Computer({ initialView = 'list', initialTab = 'overview' }: ComputerProps) {
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
  } = useComputerStore();
  const { manualTargets, fetchManualTargets } = useConnectionTargetStore();
  const [view, setView] = useState<'list' | 'detail'>(initialView);
  const [activeTab, setActiveTab] = useState<ComputerDetailTab>(initialTab);
  const [modalMode, setModalMode] = useState<'create' | 'edit' | 'duplicate' | null>(null);
  const [targetInstance, setTargetInstance] = useState<ComputerInstance | null>(null);
  const [form] = Form.useForm<{
    name: string;
    description?: string;
    copyRobotBinding?: boolean;
    connectionTargetId?: string;
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

  const handleStartStop = async (instance: ComputerInstance) => {
    try {
      if (instance.status === 'running') {
        await stopInstance(instance.id);
        message.success(t('computer.messages.stopped'));
      } else {
        await startInstance(instance.id);
        message.success(t('computer.messages.started'));
      }
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
          </>
        )}
      </Form>
    </Modal>
  );

  if (loading && instances.length === 0) {
    return <Skeleton active paragraph={{ rows: 4 }} />;
  }

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
                <Tag color={selectedInstance.connectionStatus === 'connected' ? 'green' : 'default'}>
                  {t(`computer.connection.${selectedInstance.connectionStatus}`)}
                </Tag>
                <Text type="secondary">
                  {selectedInstance.robotName
                    ? t('computer.boundRobot', { name: selectedInstance.robotName })
                    : t('computer.noRobotBound')}
                </Text>
                {selectedInstance.connectionProfile && (
                  <Text type="secondary">
                    {t('computer.connectionProfile', { name: selectedInstance.connectionProfile })}
                  </Text>
                )}
              </Space>
            </div>
            <Space wrap>
              <Button onClick={() => setView('list')}>
                {t('computer.backToList')}
              </Button>
              <Button icon={<EditOutlined />} onClick={() => openEditModal(selectedInstance)}>
                {t('computer.edit')}
              </Button>
              <Button icon={<CopyOutlined />} onClick={() => openDuplicateModal(selectedInstance)}>
                {t('computer.duplicate')}
              </Button>
              <Button
                icon={selectedInstance.status === 'running' ? <StopOutlined /> : <PlayCircleOutlined />}
                loading={loading}
                onClick={() => handleStartStop(selectedInstance)}
              >
                {selectedInstance.status === 'running' ? t('computer.stop') : t('computer.start')}
              </Button>
              <Popconfirm title={t('computer.confirmDelete')} onConfirm={() => handleDelete(selectedInstance)}>
                <Button danger icon={<DeleteOutlined />} loading={loading}>
                  {t('computer.delete')}
                </Button>
              </Popconfirm>
            </Space>
          </div>

          <Tabs
            activeKey={activeTab}
            onChange={(key) => setActiveTab(toComputerDetailTab(key))}
            items={[
              { key: 'overview', label: <><DesktopOutlined /> {t('dashboard.overview')}</>, children: <ComputerOverview instanceId={selectedInstance.id} onOpenTab={setActiveTab} /> },
              { key: 'mcp', label: <><ApiOutlined /> {t('mcp.servers')}</>, children: <McpConfig instanceId={selectedInstance.id} /> },
              { key: 'inputs', label: <><FormOutlined /> {t('inputs.title')}</>, children: <InputVariables instanceId={selectedInstance.id} /> },
              { key: 'connection', label: <><CloudServerOutlined /> {t('computer.robotConnection')}</>, children: <RobotConnectionPanel instanceId={selectedInstance.id} /> },
              { key: 'resources', label: <><DesktopOutlined /> {t('resources.title')}</>, children: <DesktopResources instanceId={selectedInstance.id} /> },
              { key: 'debug', label: <><BugOutlined /> {t('nav.debugPanel')}</>, children: <DebugPanel instanceId={selectedInstance.id} /> },
              { key: 'logs', label: <><FileTextOutlined /> {t('logs.title')}</>, children: <LogViewer instanceId={selectedInstance.id} /> },
              { key: 'runtime', label: <><SettingOutlined /> {t('settings.runtime')}</>, children: <RuntimeSettings /> },
            ]}
          />
        </Space>
        {renderComputerModal()}
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
                onDelete={() => handleDelete(instance)}
                loading={loading}
              />
            </Col>
          ))}
        </Row>
      )}
      {renderComputerModal()}
    </div>
  );
}
