import {
  App,
  Button,
  Dropdown,
  Modal,
  Space,
  Typography,
  type MenuProps,
} from 'antd';
import {
  CopyOutlined,
  DeleteOutlined,
  EditOutlined,
  FileTextOutlined,
  MoreOutlined,
  RetweetOutlined,
} from '@ant-design/icons';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ComputerInstance } from '@/stores/computerStore';

const { Text } = Typography;

interface ComputerWorkbenchMoreActionsProps {
  instance: ComputerInstance;
  loading: boolean;
  onRestart: () => void;
  onOpenLogs: () => void;
  onEdit: () => void;
  onDelete: () => Promise<void>;
}

function copyWithFallback(value: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    return navigator.clipboard.writeText(value);
  }

  const input = document.createElement('textarea');
  input.value = value;
  input.setAttribute('readonly', '');
  input.style.position = 'fixed';
  input.style.opacity = '0';
  document.body.appendChild(input);
  input.select();
  const copied = document.execCommand('copy');
  input.remove();
  return copied ? Promise.resolve() : Promise.reject(new Error('copy failed'));
}

export function ComputerWorkbenchMoreActions({
  instance,
  loading,
  onRestart,
  onOpenLogs,
  onEdit,
  onDelete,
}: ComputerWorkbenchMoreActionsProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [deleteOpen, setDeleteOpen] = useState(false);

  const handleCopyId = async () => {
    try {
      await copyWithFallback(instance.id);
      message.success(t('computer.workbench.messages.idCopied'));
    } catch {
      message.error(t('computer.workbench.messages.copyFailed'));
    }
  };

  const handleDelete = async () => {
    await onDelete();
    setDeleteOpen(false);
  };

  const items: MenuProps['items'] = [
    {
      key: 'restart',
      icon: <RetweetOutlined />,
      disabled: !instance.runtime.actions.restart.enabled,
      label: (
        <Space direction="vertical" size={0}>
          <Text>{t('computer.runtime.restart')}</Text>
          {!instance.runtime.actions.restart.enabled && (
            <Text type="secondary">
              {instance.runtime.actions.restart.disabled_reason
                ? t(`computer.runtime.actionDisabledReasons.${instance.runtime.actions.restart.disabled_reason}`)
                : t('computer.runtime.problems.actions.unavailable')}
            </Text>
          )}
        </Space>
      ),
    },
    {
      key: 'logs',
      icon: <FileTextOutlined />,
      label: t('computer.workbench.actions.viewLogs'),
    },
    {
      key: 'copy-id',
      icon: <CopyOutlined />,
      label: t('computer.workbench.actions.copyId'),
    },
    {
      key: 'edit',
      icon: <EditOutlined />,
      label: t('computer.workbench.actions.editIdentity'),
    },
    { type: 'divider' },
    {
      key: 'delete',
      icon: <DeleteOutlined />,
      danger: true,
      label: t('computer.delete'),
    },
  ];

  const handleAction: MenuProps['onClick'] = ({ key }) => {
    switch (key) {
      case 'restart':
        onRestart();
        break;
      case 'logs':
        onOpenLogs();
        break;
      case 'copy-id':
        void handleCopyId();
        break;
      case 'edit':
        onEdit();
        break;
      case 'delete':
        setDeleteOpen(true);
        break;
    }
  };

  return (
    <>
      <Dropdown trigger={['click']} menu={{ items, onClick: handleAction }}>
        <Button
          icon={<MoreOutlined />}
          aria-label={t('computer.workbench.actions.more')}
        />
      </Dropdown>
      <Modal
        title={t('computer.confirmDelete')}
        open={deleteOpen}
        okText={t('computer.delete')}
        okButtonProps={{ danger: true }}
        confirmLoading={loading}
        onOk={() => void handleDelete()}
        onCancel={() => setDeleteOpen(false)}
      >
        <Text>{t('computer.workbench.deleteDescription', { name: instance.name })}</Text>
      </Modal>
    </>
  );
}
