import { useEffect, useState } from 'react';
import { App, Button, Space, Modal, Typography, Alert, Table, Tag, Popconfirm } from 'antd';
import {
  PlusOutlined,
  ReloadOutlined,
  DeleteOutlined,
  EditOutlined,
  ClearOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useInputStore, type InputDefinition } from '@/stores/inputStore';
import { InputForm } from './InputForm';
import { InputValueEditor } from './InputValueEditor';

const { Title } = Typography;

export function InputVariables() {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    inputs,
    values,
    loading,
    error,
    fetchInputs,
    fetchValues,
    addOrUpdateInput,
    removeInput,
    setValue,
    clearValues,
  } = useInputStore();

  const [formVisible, setFormVisible] = useState(false);
  const [editingInput, setEditingInput] = useState<InputDefinition | undefined>();
  const [valueEditorVisible, setValueEditorVisible] = useState(false);
  const [editingValueId, setEditingValueId] = useState<string>('');

  useEffect(() => {
    fetchInputs();
    fetchValues();
  }, [fetchInputs, fetchValues]);

  const handleAdd = () => {
    setEditingInput(undefined);
    setFormVisible(true);
  };

  const handleEdit = (input: InputDefinition) => {
    setEditingInput(input);
    setFormVisible(true);
  };

  const handleFormSubmit = async (input: InputDefinition) => {
    try {
      await addOrUpdateInput(input);
      message.success(t('inputs.messages.saved'));
      setFormVisible(false);
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleSetValue = (id: string) => {
    setEditingValueId(id);
    setValueEditorVisible(true);
  };

  const handleValueSubmit = async (value: string) => {
    try {
      await setValue(editingValueId, value);
      message.success(t('inputs.messages.valueSet'));
      setValueEditorVisible(false);
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleClearAll = async () => {
    try {
      await clearValues();
      message.success(t('inputs.messages.valuesCleared'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const typeColors: Record<string, string> = {
    PromptString: 'blue',
    PickString: 'green',
    Command: 'orange',
  };

  const columns = [
    {
      title: t('inputs.table.id'),
      dataIndex: 'id',
      key: 'id',
      width: 150,
    },
    {
      title: t('inputs.table.type'),
      key: 'type',
      width: 120,
      render: (_: unknown, record: InputDefinition) => (
        <Tag color={typeColors[record.type]}>{record.type}</Tag>
      ),
    },
    {
      title: t('inputs.table.label'),
      dataIndex: 'label',
      key: 'label',
    },
    {
      title: t('inputs.table.currentValue'),
      key: 'value',
      width: 200,
      render: (_: unknown, record: InputDefinition) => {
        const val = values[record.id];
        if (val !== undefined && val !== null) {
          return (
            <Tag
              color="cyan"
              style={{ cursor: 'pointer' }}
              onClick={() => handleSetValue(record.id)}
            >
              {String(val).length > 30 ? String(val).substring(0, 30) + '...' : String(val)}
            </Tag>
          );
        }
        return (
          <Button type="link" size="small" onClick={() => handleSetValue(record.id)}>
            {t('inputs.setValue')}
          </Button>
        );
      },
    },
    {
      title: t('inputs.table.actions'),
      key: 'actions',
      width: 150,
      render: (_: unknown, record: InputDefinition) => (
        <Space>
          <Button
            type="text"
            size="small"
            icon={<EditOutlined />}
            onClick={() => handleEdit(record)}
          />
          <Popconfirm
            title={t('inputs.confirmRemove')}
            onConfirm={() => {
              removeInput(record.id).catch((e) => message.error(String(e)));
            }}
          >
            <Button type="text" size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('inputs.title')}</Title>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={() => { fetchInputs(); fetchValues(); }} loading={loading}>
            {t('common.refresh')}
          </Button>
          <Popconfirm title={t('inputs.confirmClearValues')} onConfirm={handleClearAll}>
            <Button icon={<ClearOutlined />} danger>
              {t('inputs.clearValues')}
            </Button>
          </Popconfirm>
          <Button type="primary" icon={<PlusOutlined />} onClick={handleAdd}>
            {t('inputs.addInput')}
          </Button>
        </Space>
      </div>

      {error && (
        <Alert message={t('common.error')} description={error} type="error" showIcon closable style={{ marginBottom: 16 }} />
      )}

      <Table
        dataSource={inputs}
        columns={columns}
        rowKey="id"
        loading={loading}
        pagination={false}
        size="middle"
      />

      <Modal
        title={editingInput ? t('inputs.editInput') : t('inputs.addInput')}
        open={formVisible}
        onCancel={() => setFormVisible(false)}
        footer={null}
        destroyOnHidden
        width={600}
      >
        <InputForm
          initialValues={editingInput}
          onSubmit={handleFormSubmit}
          onCancel={() => setFormVisible(false)}
          loading={loading}
        />
      </Modal>

      <Modal
        title={t('inputs.setValueFor', { id: editingValueId })}
        open={valueEditorVisible}
        onCancel={() => setValueEditorVisible(false)}
        footer={null}
        destroyOnHidden
        width={400}
      >
        <InputValueEditor
          inputId={editingValueId}
          inputs={inputs}
          currentValue={values[editingValueId]}
          onSubmit={handleValueSubmit}
          onCancel={() => setValueEditorVisible(false)}
        />
      </Modal>
    </div>
  );
}
