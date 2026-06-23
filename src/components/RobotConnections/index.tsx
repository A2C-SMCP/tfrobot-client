import { useEffect, useState } from 'react';
import {
  App,
  Alert,
  Button,
  Modal,
  Popconfirm,
  Space,
  Table,
  Tabs,
  Typography,
} from 'antd';
import {
  ApiOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
  UserOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { ManagerAccount } from '@/components/ManagerAccount';
import { ProfileForm } from '@/components/SmcpConnection/ProfileForm';
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
  };
}

export function RobotConnections() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    manualTargets,
    loading,
    error,
    fetchManualTargets,
    saveManualTarget,
    deleteManualTarget,
  } = useConnectionTargetStore();
  const [formOpen, setFormOpen] = useState(false);
  const [editingTarget, setEditingTarget] = useState<ManualSmcpTarget | undefined>();

  useEffect(() => {
    fetchManualTargets();
  }, [fetchManualTargets]);

  const handleSubmit = async (profile: ConnectionProfile, apiKey?: string) => {
    await saveManualTarget(profileToTarget(profile, editingTarget), apiKey);
    message.success(t('connection.messages.profileSaved'));
    setEditingTarget(undefined);
    setFormOpen(false);
  };

  const columns = [
    {
      title: t('connection.table.name'),
      dataIndex: 'name',
      key: 'name',
      render: (name: string) => <Text strong>{name}</Text>,
    },
    { title: t('connection.table.url'), dataIndex: 'url', key: 'url', ellipsis: true },
    { title: t('connection.table.office'), dataIndex: 'office_id', key: 'office_id' },
    { title: t('connection.table.computer'), dataIndex: 'computer_name', key: 'computer_name' },
    {
      title: t('connection.table.actions'),
      key: 'actions',
      width: 140,
      render: (_: unknown, record: ManualSmcpTarget) => {
        return (
          <Space>
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
        <Text type="secondary">
          Manage Robot and SMCP connection resources available to Computer instances.
        </Text>
      </div>

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
