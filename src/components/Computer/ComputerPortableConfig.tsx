import { usePageActive } from '@/components/Navigation/pageActivityState';
import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import { App, Checkbox, Space, Typography } from 'antd';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ComputerInstance } from '@/stores/computerStore';
import {
  ALL_PORTABLE_CONFIG_GROUPS,
  usePortableConfigStore,
  type PortableConfigGroup,
} from '@/stores/portableConfigStore';

const { Paragraph } = Typography;

interface ComputerPortableConfigProps {
  instance: ComputerInstance;
  exportOpen: boolean;
  onCloseExport: () => void;
}

/**
 * Export-only dialog. Importing a portable package is offered from the create-Computer
 * flow on the list page, not from inside an existing Computer.
 */
export function ComputerPortableConfig({
  instance,
  exportOpen,
  onCloseExport,
}: ComputerPortableConfigProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const active = usePageActive();
  const { exporting, exportPackage } = usePortableConfigStore();
  const [groups, setGroups] = useState<PortableConfigGroup[]>([...ALL_PORTABLE_CONFIG_GROUPS]);

  useEffect(() => {
    if (!active) onCloseExport();
  }, [active, onCloseExport]);

  const closeExport = () => {
    setGroups([...ALL_PORTABLE_CONFIG_GROUPS]);
    onCloseExport();
  };

  const handleExport = async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        defaultPath: `${instance.name}-portable.json`,
      });
      if (!path) return;
      await exportPackage(instance.id, path as string, groups);
      message.success(t('computer.portable.exportSuccess'));
      closeExport();
    } catch {
      message.error(t('computer.portable.operationFailed'));
    }
  };

  return (
    <Modal
      title={t('computer.portable.exportTitle')}
      open={exportOpen}
      okText={t('computer.portable.exportAction')}
      okButtonProps={{ loading: exporting }}
      onOk={() => void handleExport()}
      onCancel={closeExport}
    >
      <Paragraph type="secondary">{t('computer.portable.exportHint')}</Paragraph>
      <Space direction="vertical">
        {ALL_PORTABLE_CONFIG_GROUPS.map((group) => (
          <Checkbox
            key={group}
            checked={groups.includes(group)}
            onChange={(event) => {
              setGroups((current) => (
                event.target.checked
                  ? [...current, group]
                  : current.filter((item) => item !== group)
              ));
            }}
          >
            {t(`computer.portable.groups.${group}`)}
          </Checkbox>
        ))}
      </Space>
    </Modal>
  );
}
