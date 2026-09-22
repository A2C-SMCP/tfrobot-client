import { usePageActive } from '@/components/Navigation/pageActivityState';
import { PageModal as Modal } from '@/components/Navigation/PageOverlays';
import { Alert, App, Button, Checkbox, Input, Space, Tag, Typography } from 'antd';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ComputerInstance } from '@/stores/computerStore';
import { useComputerStore } from '@/stores/computerStore';
import {
  ALL_PORTABLE_CONFIG_GROUPS,
  usePortableConfigStore,
  type PortableConfigGroup,
  type PortablePackagePreview,
} from '@/stores/portableConfigStore';

const { Text, Paragraph } = Typography;

function marketplaceSourceLabel(source: unknown): string {
  if (source && typeof source === 'object') {
    const value = source as Record<string, unknown>;
    if (typeof value.url === 'string') return value.url;
    if (typeof value.path === 'string') return value.path;
  }
  return '-';
}

interface ComputerPortableConfigProps {
  instance: ComputerInstance;
  exportOpen: boolean;
  importOpen: boolean;
  onCloseExport: () => void;
  onCloseImport: () => void;
}

export function ComputerPortableConfig({
  instance,
  exportOpen,
  importOpen,
  onCloseExport,
  onCloseImport,
}: ComputerPortableConfigProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const active = usePageActive();
  const { exporting, previewing, committing, exportPackage, previewImport, commitImport } =
    usePortableConfigStore();
  const fetchInstances = useComputerStore((state) => state.fetchInstances);

  const [groups, setGroups] = useState<PortableConfigGroup[]>([...ALL_PORTABLE_CONFIG_GROUPS]);
  const [preview, setPreview] = useState<PortablePackagePreview | null>(null);
  const [importPath, setImportPath] = useState<string | null>(null);
  const [finalName, setFinalName] = useState('');

  useEffect(() => {
    if (!active) {
      onCloseExport();
      onCloseImport();
    }
  }, [active, onCloseExport, onCloseImport]);

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

  const chooseImportFile = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const path = await open({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        multiple: false,
      });
      if (!path) return;
      const result = await previewImport(path as string);
      setImportPath(path as string);
      setPreview(result);
      setFinalName(result.finalName);
    } catch (error) {
      message.error(String(error));
    }
  };

  const handleCommit = async () => {
    if (!importPath || !preview) return;
    const trimmed = finalName.trim();
    if (!trimmed) {
      message.error(t('computer.portable.nameRequired'));
      return;
    }
    try {
      const result = await commitImport(importPath, trimmed);
      message.success(t('computer.portable.importSuccess', { name: result.name }));
      await fetchInstances();
      setPreview(null);
      setImportPath(null);
      onCloseImport();
    } catch (error) {
      message.error(String(error));
    }
  };

  const incompatible = !preview?.versionCompatible;

  return (
    <>
      <Modal
        title={t('computer.portable.exportTitle')}
        open={exportOpen}
        okText={t('computer.portable.exportAction')}
        okButtonProps={{ loading: exporting }}
        onOk={() => void handleExport()}
        onCancel={closeExport}
      >
        <Paragraph type="secondary">
          {t('computer.portable.exportHint')}
        </Paragraph>
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

      <Modal
        title={t('computer.portable.importTitle')}
        open={importOpen}
        okText={t('computer.portable.importAction')}
        okButtonProps={{ loading: committing, disabled: incompatible || !preview }}
        onOk={() => void handleCommit()}
        onCancel={() => {
          setPreview(null);
          setImportPath(null);
          onCloseImport();
        }}
      >
        {!preview ? (
          <Button loading={previewing} onClick={() => void chooseImportFile()}>
            {t('computer.portable.chooseFile')}
          </Button>
        ) : (
          <Space direction="vertical" style={{ width: '100%' }}>
            {!preview.versionCompatible && (
              <Alert
                type="error"
                showIcon
                message={preview.versionMessage ?? t('computer.portable.incompatibleVersion')}
              />
            )}
            <Space direction="vertical" size={0}>
              <Text type="secondary">{t('computer.portable.originalName')}</Text>
              <Text strong>{preview.originalName}</Text>
            </Space>
            <Space direction="vertical" size={4} style={{ width: '100%' }}>
              <Text>{t('computer.portable.finalName')}</Text>
              <Input value={finalName} onChange={(event) => setFinalName(event.target.value)} />
              {preview.nameConflict && (
                <Text type="danger">{t('computer.portable.nameConflicts')}</Text>
              )}
            </Space>
            <Space direction="vertical" size={0}>
              <Text type="secondary">{t('computer.portable.includedGroups')}</Text>
              {preview.sections
                .filter((section) => section.status !== 'missing')
                .map((section) => (
                  <Text key={section.group}>
                    {t(`computer.portable.groups.${section.group}`)}
                  </Text>
                ))}
            </Space>
            {preview.marketplaces.length > 0 && (
              <Space direction="vertical" size={4}>
                <Text type="secondary">{t('computer.portable.marketplaces')}</Text>
                {preview.marketplaces.map((marketplace) => (
                  <Tag key={marketplace.name}>
                    {marketplace.name} · {marketplaceSourceLabel(marketplace.source)}
                  </Tag>
                ))}
              </Space>
            )}
            {preview.installedPlugins.length > 0 && (
              <Space direction="vertical" size={4}>
                <Text type="secondary">{t('computer.portable.plugins')}</Text>
                {preview.installedPlugins.map((plugin) => (
                  <Text key={plugin}>{plugin}</Text>
                ))}
              </Space>
            )}
            {(preview.marketplaces.length > 0 || preview.installedPlugins.length > 0) && (
              <Alert
                type="warning"
                showIcon
                message={t('computer.portable.externalCodeWarning')}
              />
            )}
          </Space>
        )}
      </Modal>
    </>
  );
}
