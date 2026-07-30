import {
  Button,
  Dropdown,
  Modal,
  Space,
  Typography,
  type MenuProps,
} from 'antd';
import {
  DeleteOutlined,
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
  onDelete: () => Promise<void>;
}

export function ComputerWorkbenchMoreActions({
  instance,
  loading,
  onRestart,
  onDelete,
}: ComputerWorkbenchMoreActionsProps) {
  const { t } = useTranslation();
  const [deleteOpen, setDeleteOpen] = useState(false);

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
