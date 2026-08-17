import { useEffect, useState } from 'react';
import { App, Button, Space, Modal, Typography, Alert, Table, Popconfirm } from 'antd';
import {
  ReloadOutlined,
  ClearOutlined,
  DeleteOutlined,
  EditOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useInputStore, type InputDefinition } from '@/stores/inputStore';
import { InputValueEditor } from './InputValueEditor';

const { Title } = Typography;

interface InputVariablesProps {
  instanceId: string;
}

export function InputVariables({ instanceId }: InputVariablesProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const {
    inputs,
    values,
    loading,
    error,
    valuesLoading,
    valuesLoadedInstanceId,
    valuesError,
    fetchInputs,
    fetchValues,
    setValue,
    removeValue,
    clearValues,
  } = useInputStore();

  const [valueEditorVisible, setValueEditorVisible] = useState(false);
  const [editingValueId, setEditingValueId] = useState('');
  const valuesReady = valuesLoadedInstanceId === instanceId && !valuesLoading && !valuesError;

  useEffect(() => {
    fetchInputs(instanceId);
    fetchValues(instanceId);
  }, [fetchInputs, fetchValues, instanceId]);

  useEffect(() => {
    setValueEditorVisible(false);
    setEditingValueId('');
  }, [instanceId]);

  useEffect(() => {
    if (!valuesReady) {
      setValueEditorVisible(false);
      setEditingValueId('');
    }
  }, [valuesReady]);

  const handleSetValue = (id: string) => {
    setEditingValueId(id);
    setValueEditorVisible(true);
  };

  const handleValueSubmit = async (value: string) => {
    if (!valuesReady || !editingValueId) return;
    try {
      await setValue(instanceId, editingValueId, value);
      message.success(t('inputs.messages.valueSet'));
      setValueEditorVisible(false);
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleClearAll = async () => {
    try {
      await clearValues(instanceId);
      message.success(t('inputs.messages.valuesCleared'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handleRemoveValue = async (id: string) => {
    try {
      await removeValue(instanceId, id);
      message.success(t('inputs.messages.valueCleared'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const columns = [
    {
      title: t('inputs.table.id'),
      dataIndex: 'id',
      key: 'id',
      width: 150,
    },
    {
      title: t('inputs.table.label'),
      key: 'label',
      render: (_: unknown, record: InputDefinition) => record.label || record.id,
    },
    {
      title: t('inputs.table.storageMode'),
      key: 'storageMode',
      width: 190,
      render: (_: unknown, record: InputDefinition) => {
        if (record.type === 'Command') return '—';
        return record.type === 'PromptString' && record.password
          ? t('inputs.secretMode')
          : t('inputs.nonSecretMode');
      },
    },
    {
      title: t('inputs.table.currentValue'),
      key: 'value',
      width: 200,
      render: (_: unknown, record: InputDefinition) => {
        if (record.type === 'Command') return t('inputs.runtimeCommand');
        if (valuesLoading) return t('inputs.valuesLoading');
        if (!valuesReady) return t('inputs.valuesUnavailable');
        const stored = values[record.id];
        let displayValue: string;
        switch (stored?.status) {
          case 'configured':
            displayValue = record.type === 'PromptString' && record.password
              ? t('inputs.configuredSecret')
              : String(stored.value ?? '');
            break;
          case 'using_default':
            displayValue = t('inputs.status.usingDefault');
            break;
          case 'first_option':
            displayValue = t('inputs.status.firstOption');
            break;
          case 'invalid_selection':
            displayValue = t('inputs.status.invalidSelection', { value: String(stored.value ?? '') });
            break;
          default:
            displayValue = t('inputs.notConfigured');
        }
        return stored?.status === 'configured' && displayValue.length > 30
          ? `${displayValue.substring(0, 30)}...`
          : displayValue;
      },
    },
    {
      title: t('inputs.table.actions'),
      key: 'actions',
      width: 210,
      render: (_: unknown, record: InputDefinition) => {
        if (record.type === 'Command') return '—';
        const configured = valuesReady && values[record.id]?.configured === true;
        return (
          <Space size="small">
            <Button
              type="link"
              size="small"
              icon={<EditOutlined />}
              aria-label={t('inputs.setValueFor', { id: record.id })}
              disabled={!valuesReady}
              onClick={() => handleSetValue(record.id)}
            >
              {configured ? t('inputs.editValue') : t('inputs.setValue')}
            </Button>
            <Popconfirm
              title={t('inputs.confirmClearValue', { id: record.id })}
              onConfirm={() => handleRemoveValue(record.id)}
              disabled={!configured}
            >
              <Button
                type="link"
                size="small"
                danger
                disabled={!configured}
                icon={<DeleteOutlined />}
                aria-label={t('inputs.clearValueFor', { id: record.id })}
              >
                {t('inputs.clearValue')}
              </Button>
            </Popconfirm>
          </Space>
        );
      },
    },
  ];

  return (
    <div>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: 12,
          marginBottom: 16,
        }}
      >
        <Title level={4} style={{ margin: 0 }}>{t('inputs.title')}</Title>
        <Space wrap>
          <Button icon={<ReloadOutlined />} onClick={() => {
            fetchInputs(instanceId);
            fetchValues(instanceId);
          }} loading={loading || valuesLoading}>
            {t('common.refresh')}
          </Button>
          <Popconfirm title={t('inputs.confirmClearValues')} onConfirm={handleClearAll}>
            <Button icon={<ClearOutlined />} danger disabled={!valuesReady}>
              {t('inputs.clearValues')}
            </Button>
          </Popconfirm>
        </Space>
      </div>

      {(error || valuesError) && (
        <Alert message={t('common.error')} description={error || valuesError} type="error" showIcon closable style={{ marginBottom: 16 }} />
      )}

      <Table
        dataSource={inputs}
        columns={columns}
        rowKey="id"
        loading={loading || valuesLoading}
        pagination={false}
        size="middle"
        scroll={{ x: 'max-content' }}
      />

      {valueEditorVisible && (
        <Modal
          title={t('inputs.setValueFor', { id: editingValueId })}
          open
          onCancel={() => setValueEditorVisible(false)}
          footer={null}
          destroyOnHidden
          width={400}
        >
          <InputValueEditor
            key={`${instanceId}:${editingValueId}`}
            inputId={editingValueId}
            inputs={inputs}
            currentValue={values[editingValueId]?.value}
            disabled={!valuesReady}
            onSubmit={handleValueSubmit}
            onCancel={() => setValueEditorVisible(false)}
          />
        </Modal>
      )}

    </div>
  );
}
