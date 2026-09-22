import { usePageActive } from '@/components/Navigation/pageActivityState';
import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import {
  Button,
  Dropdown,
  Space,
  Typography,
  type MenuProps,
} from 'antd';
import {
  DeleteOutlined,
  ExportOutlined,
  ImportOutlined,
  MoreOutlined,
  RetweetOutlined,
} from '@ant-design/icons';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ComputerInstance } from '@/stores/computerStore';
import { ComputerPortableConfig } from './ComputerPortableConfig';

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
  const [exportOpen, setExportOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const active = usePageActive();
  useEffect(() => {
    if (!active) {
      setDeleteOpen(false);
      setExportOpen(false);
      setImportOpen(false);
    }
  }, [active]);

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
      key: 'export',
      icon: <ExportOutlined />,
      label: t('computer.portable.exportConfig'),
    },
    {
      key: 'import',
      icon: <ImportOutlined />,
      label: t('computer.portable.importConfig'),
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
      case 'export':
        setExportOpen(true);
        break;
      case 'import':
        setImportOpen(true);
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
      <ComputerPortableConfig
        instance={instance}
        exportOpen={exportOpen}
        importOpen={importOpen}
        onCloseExport={() => setExportOpen(false)}
        onCloseImport={() => setImportOpen(false)}
      />
    </>
  );
}
