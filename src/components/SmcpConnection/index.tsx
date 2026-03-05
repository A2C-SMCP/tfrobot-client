import { useEffect, useState } from 'react';
import { Button, Space, Typography, message, Alert, Card, Table, Tag, Popconfirm, Modal, Descriptions } from 'antd';
import {
  PlusOutlined,
  ReloadOutlined,
  DeleteOutlined,
  EditOutlined,
  LinkOutlined,
  DisconnectOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useConnectionStore, type ConnectionProfile } from '@/stores/connectionStore';
import { ProfileForm } from './ProfileForm';

const { Title, Text } = Typography;

export function SmcpConnection() {
  const { t } = useTranslation();
  const {
    profiles,
    status,
    loading,
    error,
    fetchProfiles,
    fetchStatus,
    saveProfile,
    deleteProfile,
    connect,
    disconnect,
  } = useConnectionStore();

  const [formVisible, setFormVisible] = useState(false);
  const [editingProfile, setEditingProfile] = useState<ConnectionProfile | undefined>();

  useEffect(() => {
    fetchProfiles();
    fetchStatus();
  }, [fetchProfiles, fetchStatus]);

  const handleAdd = () => {
    setEditingProfile(undefined);
    setFormVisible(true);
  };

  const handleEdit = (profile: ConnectionProfile) => {
    setEditingProfile(profile);
    setFormVisible(true);
  };

  const handleFormSubmit = async (profile: ConnectionProfile, apiKey?: string) => {
    try {
      await saveProfile(profile, apiKey);
      message.success(t('connection.messages.profileSaved'));
      setFormVisible(false);
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleConnect = async (profileName: string) => {
    try {
      await connect(profileName);
      message.success(t('connection.messages.connected'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleDisconnect = async () => {
    try {
      await disconnect();
      message.success(t('connection.messages.disconnected'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const columns = [
    {
      title: t('connection.table.name'),
      dataIndex: 'name',
      key: 'name',
      render: (name: string) => {
        const isActive = status.connected && status.profile_name === name;
        return (
          <Space>
            {isActive && <CheckCircleOutlined style={{ color: '#52c41a' }} />}
            <Text strong={isActive}>{name}</Text>
          </Space>
        );
      },
    },
    {
      title: t('connection.table.url'),
      dataIndex: 'url',
      key: 'url',
      ellipsis: true,
    },
    {
      title: t('connection.table.office'),
      dataIndex: 'office_id',
      key: 'office_id',
    },
    {
      title: t('connection.table.computer'),
      dataIndex: 'computer_name',
      key: 'computer_name',
    },
    {
      title: t('connection.table.actions'),
      key: 'actions',
      width: 200,
      render: (_: unknown, record: ConnectionProfile) => {
        const isActive = status.connected && status.profile_name === record.name;
        return (
          <Space>
            {isActive ? (
              <Button
                size="small"
                danger
                icon={<DisconnectOutlined />}
                onClick={handleDisconnect}
                loading={loading}
              >
                {t('connection.disconnect')}
              </Button>
            ) : (
              <Button
                size="small"
                type="primary"
                icon={<LinkOutlined />}
                onClick={() => handleConnect(record.name)}
                loading={loading}
              >
                {t('connection.connect')}
              </Button>
            )}
            <Button
              type="text"
              size="small"
              icon={<EditOutlined />}
              onClick={() => handleEdit(record)}
            />
            <Popconfirm
              title={t('connection.confirmDelete')}
              onConfirm={() => deleteProfile(record.name).catch((e) => message.error(String(e)))}
            >
              <Button type="text" size="small" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </Space>
        );
      },
    },
  ];

  return (
    <div>
      {/* Connection Status Card */}
      <Card
        size="small"
        style={{ marginBottom: 16 }}
        title={
          <Space>
            {status.connected ? (
              <Tag icon={<CheckCircleOutlined />} color="success">{t('connection.connected')}</Tag>
            ) : (
              <Tag icon={<CloseCircleOutlined />} color="default">{t('connection.disconnected')}</Tag>
            )}
            {t('connection.statusTitle')}
          </Space>
        }
      >
        {status.connected ? (
          <Descriptions size="small" column={2}>
            <Descriptions.Item label={t('connection.table.url')}>{status.url}</Descriptions.Item>
            <Descriptions.Item label={t('connection.table.office')}>{status.office_id}</Descriptions.Item>
            <Descriptions.Item label={t('connection.table.computer')}>{status.computer_name}</Descriptions.Item>
            <Descriptions.Item label={t('connection.connectedAt')}>
              {status.connected_at ? new Date(status.connected_at).toLocaleString() : '-'}
            </Descriptions.Item>
          </Descriptions>
        ) : (
          <Text type="secondary">{t('connection.notConnected')}</Text>
        )}
      </Card>

      {/* Profile List */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('connection.profiles')}</Title>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={() => { fetchProfiles(); fetchStatus(); }} loading={loading}>
            {t('common.refresh')}
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={handleAdd}>
            {t('connection.addProfile')}
          </Button>
        </Space>
      </div>

      {error && (
        <Alert message={t('common.error')} description={error} type="error" showIcon closable style={{ marginBottom: 16 }} />
      )}

      <Table
        dataSource={profiles}
        columns={columns}
        rowKey="name"
        loading={loading}
        pagination={false}
        size="middle"
      />

      <Modal
        title={editingProfile ? t('connection.editProfile') : t('connection.addProfile')}
        open={formVisible}
        onCancel={() => setFormVisible(false)}
        footer={null}
        destroyOnHidden
        width={600}
      >
        <ProfileForm
          initialValues={editingProfile}
          onSubmit={handleFormSubmit}
          onCancel={() => setFormVisible(false)}
          loading={loading}
        />
      </Modal>
    </div>
  );
}
