import { useEffect, useState } from 'react';
import { App, Alert, Button, Modal, Popconfirm, Space, Table, Typography } from 'antd';
import { DeleteOutlined, EditOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useInputStore, type InputEntry } from '@/stores/inputStore';
import { InputEntryEditor } from './InputEntryEditor';

const { Title } = Typography;

interface InputVariablesProps {
  instanceId: string;
}

export function InputVariables({ instanceId }: InputVariablesProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    entries,
    entriesLoading,
    entriesLoadedInstanceId,
    entriesError,
    fetchEntries,
    upsertEntry,
    deleteEntry,
  } = useInputStore();
  const [editorOpen, setEditorOpen] = useState(false);
  const [editing, setEditing] = useState<InputEntry>();
  const ready = entriesLoadedInstanceId === instanceId && !entriesLoading && !entriesError;

  useEffect(() => {
    fetchEntries(instanceId);
  }, [fetchEntries, instanceId]);

  useEffect(() => {
    setEditorOpen(false);
    setEditing(undefined);
  }, [instanceId]);

  const openCreate = () => {
    setEditing(undefined);
    setEditorOpen(true);
  };
  const openEdit = (entry: InputEntry) => {
    setEditing(entry);
    setEditorOpen(true);
  };
  const handleSubmit = async (key: string, value: string | undefined, secret: boolean) => {
    await upsertEntry(instanceId, key, value, secret);
    message.success(t(editing ? 'inputs.messages.entryUpdated' : 'inputs.messages.entryCreated'));
    setEditorOpen(false);
  };
  const handleDelete = async (key: string) => {
    try {
      await deleteEntry(instanceId, key);
      message.success(t('inputs.messages.entryDeleted'));
    } catch (cause) {
      message.error(String(cause));
    }
  };

  const columns = [
    {
      title: t('inputs.entry.key'),
      dataIndex: 'key',
      key: 'key',
    },
    {
      title: t('inputs.table.storageMode'),
      key: 'storageMode',
      width: 220,
      render: (_: unknown, entry: InputEntry) => entry.secret
        ? t('inputs.secretMode')
        : t('inputs.nonSecretMode'),
    },
    {
      title: t('inputs.entry.value'),
      key: 'value',
      render: (_: unknown, entry: InputEntry) => entry.secret
        ? t('inputs.configuredSecret')
        : String(entry.value ?? ''),
    },
    {
      title: t('inputs.table.actions'),
      key: 'actions',
      width: 190,
      render: (_: unknown, entry: InputEntry) => (
        <Space size="small">
          <Button
            type="link"
            size="small"
            icon={<EditOutlined />}
            aria-label={t('inputs.entry.editFor', { key: entry.key })}
            onClick={() => openEdit(entry)}
          >
            {t('common.edit')}
          </Button>
          <Popconfirm
            title={t('inputs.entry.confirmDelete', { key: entry.key })}
            onConfirm={() => handleDelete(entry.key)}
          >
            <Button
              type="link"
              size="small"
              danger
              icon={<DeleteOutlined />}
              aria-label={t('inputs.entry.deleteFor', { key: entry.key })}
            >
              {t('common.delete')}
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12, marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('inputs.title')}</Title>
        <Space wrap>
          <Button icon={<ReloadOutlined />} loading={entriesLoading} onClick={() => fetchEntries(instanceId)}>
            {t('common.refresh')}
          </Button>
          <Button type="primary" icon={<PlusOutlined />} disabled={!ready} onClick={openCreate}>
            {t('inputs.entry.add')}
          </Button>
        </Space>
      </div>

      {entriesError && (
        <Alert message={t('common.error')} description={entriesError} type="error" showIcon style={{ marginBottom: 16 }} />
      )}

      <Table
        dataSource={entries}
        columns={columns}
        rowKey="key"
        loading={entriesLoading}
        pagination={false}
        locale={{ emptyText: t('inputs.entry.empty') }}
      />

      {editorOpen && (
        <Modal
          title={t(editing ? 'inputs.entry.edit' : 'inputs.entry.add')}
          open
          onCancel={() => setEditorOpen(false)}
          footer={null}
          destroyOnHidden
          width={440}
        >
          <InputEntryEditor entry={editing} onSubmit={handleSubmit} onCancel={() => setEditorOpen(false)} />
        </Modal>
      )}
    </div>
  );
}
