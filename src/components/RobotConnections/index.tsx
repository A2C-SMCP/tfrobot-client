import { useEffect, useMemo, useState } from 'react';
import {
  App,
  Alert,
  Button,
  Card,
  Descriptions,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd';
import {
  ApiOutlined,
  CheckCircleOutlined,
  DeleteOutlined,
  EditOutlined,
  LinkOutlined,
  PlusOutlined,
  UserOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { ManagerAccount } from '@/components/ManagerAccount';
import { ProfileForm } from '@/components/SmcpConnection/ProfileForm';
import { useComputerStore } from '@/stores/computerStore';
import { useConnectionStore } from '@/stores/connectionStore';
import {
  useConnectionTargetStore,
  type ManualSmcpTarget,
} from '@/stores/connectionTargetStore';
import type { ConnectionProfile } from '@/stores/connectionStore';

const { Title, Text } = Typography;

function targetToProfile(target: ManualSmcpTarget): ConnectionProfile {
  return {
    name: target.name,
    url: target.url,
    namespace: target.namespace,
    office_id: target.office_id,
    computer_name: target.computer_name,
    headers: target.headers,
    auto_connect: target.auto_connect,
    auto_reconnect: target.auto_reconnect,
  };
}

function profileToTarget(profile: ConnectionProfile, current?: ManualSmcpTarget): ManualSmcpTarget {
  return {
    id: current?.id ?? '',
    name: profile.name,
    url: profile.url,
    namespace: profile.namespace,
    office_id: profile.office_id,
    computer_name: profile.computer_name,
    headers: profile.headers,
    auto_connect: profile.auto_connect,
    auto_reconnect: profile.auto_reconnect,
  };
}

export function RobotConnections() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const { instances, selectedInstanceId, fetchInstances, selectInstance } = useComputerStore();
  const { getStatus, fetchStatus } = useConnectionStore();
  const {
    manualTargets,
    loading,
    error,
    fetchManualTargets,
    saveManualTarget,
    deleteManualTarget,
    connectTarget,
  } = useConnectionTargetStore();
  const [formOpen, setFormOpen] = useState(false);
  const [editingTarget, setEditingTarget] = useState<ManualSmcpTarget | undefined>();

  useEffect(() => {
    fetchInstances();
    fetchManualTargets();
  }, [fetchInstances, fetchManualTargets]);

  useEffect(() => {
    if (selectedInstanceId) {
      fetchStatus(selectedInstanceId);
    }
  }, [fetchStatus, selectedInstanceId]);

  const selectedComputer = useMemo(
    () => instances.find((instance) => instance.id === selectedInstanceId),
    [instances, selectedInstanceId],
  );
  const status = selectedInstanceId ? getStatus(selectedInstanceId) : { connected: false };

  const handleSubmit = async (profile: ConnectionProfile, apiKey?: string) => {
    await saveManualTarget(profileToTarget(profile, editingTarget), apiKey);
    message.success(t('connection.messages.profileSaved'));
    setEditingTarget(undefined);
    setFormOpen(false);
  };

  const handleConnect = async (target: ManualSmcpTarget) => {
    if (!selectedInstanceId) {
      message.error('Select a Computer first');
      return;
    }
    await connectTarget(selectedInstanceId, target.id);
    message.success(t('connection.messages.connected'));
  };

  const columns = [
    {
      title: t('connection.table.name'),
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: ManualSmcpTarget) => {
        const active = status.connected && status.target_id === record.id;
        return (
          <Space>
            {active && <CheckCircleOutlined style={{ color: '#52c41a' }} />}
            <Text strong={active}>{name}</Text>
          </Space>
        );
      },
    },
    { title: t('connection.table.url'), dataIndex: 'url', key: 'url', ellipsis: true },
    { title: t('connection.table.office'), dataIndex: 'office_id', key: 'office_id' },
    { title: t('connection.table.computer'), dataIndex: 'computer_name', key: 'computer_name' },
    {
      title: t('connection.table.actions'),
      key: 'actions',
      width: 230,
      render: (_: unknown, record: ManualSmcpTarget) => {
        const active = status.connected && status.target_id === record.id;
        return (
          <Space>
            {active ? (
              <Tag color="success">{t('connection.connected')}</Tag>
            ) : (
              <Button
                size="small"
                type="primary"
                icon={<LinkOutlined />}
                loading={loading}
                onClick={() => handleConnect(record).catch((e) => message.error(String(e)))}
              >
                {t('connection.connect')}
              </Button>
            )}
            <Button
              type="text"
              size="small"
              icon={<EditOutlined />}
              onClick={() => {
                setEditingTarget(record);
                setFormOpen(true);
              }}
            />
            <Popconfirm
              title={t('connection.confirmDelete')}
              onConfirm={() =>
                deleteManualTarget(record.id).catch((e) => message.error(String(e)))
              }
            >
              <Button type="text" size="small" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </Space>
        );
      },
    },
  ];

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div>
        <Title level={3} style={{ marginBottom: 4 }}>
          Robot Connections
        </Title>
        <Text type="secondary">Manage Manager Robots and local manual SMCP targets.</Text>
      </div>

      <Card size="small">
        <Space direction="vertical" size={12} style={{ width: '100%' }}>
          <Space wrap>
            <Text strong>Target Computer</Text>
            <Select
              style={{ minWidth: 260 }}
              value={selectedInstanceId ?? undefined}
              placeholder="Select Computer"
              onChange={selectInstance}
              options={instances.map((instance) => ({ value: instance.id, label: instance.name }))}
            />
            <Button onClick={() => fetchInstances()}>{t('common.refresh')}</Button>
          </Space>
          {selectedComputer && (
            <Descriptions size="small" column={3}>
              <Descriptions.Item label="Status">
                <Tag color={selectedComputer.connectionStatus === 'connected' ? 'green' : 'default'}>
                  {selectedComputer.connectionStatus}
                </Tag>
              </Descriptions.Item>
              <Descriptions.Item label="Target">
                {status.target_name ?? status.profile_name ?? '-'}
              </Descriptions.Item>
              <Descriptions.Item label="Source">{status.source_type ?? '-'}</Descriptions.Item>
            </Descriptions>
          )}
        </Space>
      </Card>

      <Tabs
        items={[
          {
            key: 'manager',
            label: (
              <>
                <UserOutlined /> Manager Robots
              </>
            ),
            children: <ManagerAccount />,
          },
          {
            key: 'manual',
            label: (
              <>
                <ApiOutlined /> Manual SMCP
              </>
            ),
            children: (
              <Space direction="vertical" size={16} style={{ width: '100%' }}>
                {error && <Alert type="error" showIcon message={error} />}
                <Space>
                  <Button
                    type="primary"
                    icon={<PlusOutlined />}
                    onClick={() => {
                      setEditingTarget(undefined);
                      setFormOpen(true);
                    }}
                  >
                    {t('connection.addProfile')}
                  </Button>
                  <Button onClick={() => fetchManualTargets()}>{t('common.refresh')}</Button>
                </Space>
                <Table
                  rowKey="id"
                  columns={columns}
                  dataSource={manualTargets}
                  loading={loading}
                  pagination={false}
                />
              </Space>
            ),
          },
        ]}
      />

      <Modal
        open={formOpen}
        title={editingTarget ? t('connection.editProfile') : t('connection.addProfile')}
        footer={null}
        onCancel={() => {
          setEditingTarget(undefined);
          setFormOpen(false);
        }}
        destroyOnHidden
      >
        <ProfileForm
          initialValues={editingTarget ? targetToProfile(editingTarget) : undefined}
          onSubmit={handleSubmit}
          onCancel={() => {
            setEditingTarget(undefined);
            setFormOpen(false);
          }}
          loading={loading}
        />
      </Modal>
    </Space>
  );
}
