import { useEffect } from 'react';
import { App, Alert, Button, Card, Descriptions, Empty, Form, Input, List, Space, Tag, Typography } from 'antd';
import { CloudDownloadOutlined, DeleteOutlined, PauseCircleOutlined, PlayCircleOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useSkillStore } from '@/stores/skillStore';

const { Text, Title } = Typography;

interface MarketplaceTabProps {
  instanceId: string;
}

interface MarketplaceFormValues {
  name: string;
  gitUrl: string;
}

interface PluginFormValues {
  marketplace: string;
  plugin: string;
}

export function MarketplaceTab({ instanceId }: MarketplaceTabProps) {
  const { t } = useTranslation();
  const { message } = App.useApp();
  const [marketplaceForm] = Form.useForm<MarketplaceFormValues>();
  const [pluginForm] = Form.useForm<PluginFormValues>();
  const {
    capabilities,
    marketplaces,
    plugins,
    loadingMarketplace,
    marketplaceError,
    fetchMarketplaceGovernance,
    addMarketplace,
    refreshMarketplace,
    removeMarketplace,
    installPlugin,
    enablePlugin,
    disablePlugin,
    uninstallPlugin,
  } = useSkillStore();

  useEffect(() => {
    fetchMarketplaceGovernance(instanceId);
  }, [fetchMarketplaceGovernance, instanceId]);

  const supported = capabilities?.computerLifecycleApiAvailable ?? false;

  const handleAddMarketplace = async () => {
    const values = await marketplaceForm.validateFields();
    try {
      await addMarketplace(instanceId, values);
      marketplaceForm.resetFields();
      message.success(t('marketplace.messages.added'));
    } catch (e) {
      message.error(String(e));
    }
  };

  const handlePluginAction = async (
    action: 'install' | 'enable' | 'disable' | 'uninstall',
  ) => {
    const request = await pluginForm.validateFields();
    try {
      if (action === 'install') await installPlugin(instanceId, request);
      if (action === 'enable') await enablePlugin(instanceId, request);
      if (action === 'disable') await disablePlugin(instanceId, request);
      if (action === 'uninstall') await uninstallPlugin(instanceId, request);
      message.success(t(`marketplace.messages.${action}`));
    } catch (e) {
      message.error(String(e));
    }
  };

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 16 }}>
        <Title level={4} style={{ margin: 0 }}>{t('marketplace.title')}</Title>
        <Button
          icon={<ReloadOutlined />}
          loading={loadingMarketplace}
          onClick={() => fetchMarketplaceGovernance(instanceId)}
        >
          {t('common.refresh')}
        </Button>
      </div>

      {marketplaceError && (
        <Alert type="error" showIcon message={t('common.error')} description={marketplaceError} />
      )}

      <Alert
        type={supported ? 'success' : 'warning'}
        showIcon
        message={supported ? t('marketplace.supported') : t('marketplace.unsupported')}
        description={capabilities?.reason}
      />

      <Descriptions size="small" bordered column={1}>
        <Descriptions.Item label={t('marketplace.scope')}>
          <Text code>{instanceId}</Text>
        </Descriptions.Item>
        <Descriptions.Item label={t('marketplace.requiredApis')}>
          <Space wrap>
            {(capabilities?.requiredSdkApis ?? []).map((api) => (
              <Tag key={api}>{api}</Tag>
            ))}
          </Space>
        </Descriptions.Item>
      </Descriptions>

      <Card size="small" title={t('marketplace.marketplacesTitle')}>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          {marketplaces.length === 0 ? (
            <Empty description={t('marketplace.emptyMarketplaces')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : (
            <List
              size="small"
              dataSource={marketplaces}
              renderItem={(marketplace) => (
                <List.Item
                  actions={[
                    <Button key="refresh" size="small" icon={<ReloadOutlined />} disabled={!supported} loading={loadingMarketplace} onClick={() => refreshMarketplace(instanceId, marketplace.name)}>
                      {t('marketplace.actions.refreshMarketplace')}
                    </Button>,
                    <Button key="remove" size="small" danger icon={<DeleteOutlined />} disabled={!supported} loading={loadingMarketplace} onClick={() => removeMarketplace(instanceId, marketplace.name)}>
                      {t('marketplace.actions.removeMarketplace')}
                    </Button>,
                  ]}
                >
                  <List.Item.Meta
                    title={<Space><Text strong>{marketplace.name}</Text><Tag>{marketplace.status}</Tag></Space>}
                    description={marketplace.message ?? marketplace.gitUrl ?? t('marketplace.sdkOwnedState')}
                  />
                </List.Item>
              )}
            />
          )}
          <Form form={marketplaceForm} layout="vertical">
            <Form.Item
              name="name"
              label={t('marketplace.form.name')}
              rules={[{ required: true, whitespace: true, message: t('marketplace.form.nameRequired') }]}
            >
              <Input disabled={!supported} />
            </Form.Item>
            <Form.Item
              name="gitUrl"
              label={t('marketplace.form.gitUrl')}
              rules={[{ required: true, whitespace: true, message: t('marketplace.form.gitUrlRequired') }]}
            >
              <Input disabled={!supported} />
            </Form.Item>
            <Button type="primary" icon={<PlusOutlined />} disabled={!supported} loading={loadingMarketplace} onClick={handleAddMarketplace}>
              {t('common.add')}
            </Button>
          </Form>
        </Space>
      </Card>

      <Card size="small" title={t('marketplace.pluginsTitle')}>
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          {plugins.length === 0 ? (
            <Empty description={t('marketplace.emptyPlugins')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : (
            <List
              size="small"
              dataSource={plugins}
              renderItem={(plugin) => (
                <List.Item
                  actions={[
                    <Button key="enable" size="small" icon={<PlayCircleOutlined />} disabled={!supported || plugin.enabled} loading={loadingMarketplace} onClick={() => enablePlugin(instanceId, plugin)}>
                      {t('marketplace.actions.enablePlugin')}
                    </Button>,
                    <Button key="disable" size="small" icon={<PauseCircleOutlined />} disabled={!supported || !plugin.enabled} loading={loadingMarketplace} onClick={() => disablePlugin(instanceId, plugin)}>
                      {t('marketplace.actions.disablePlugin')}
                    </Button>,
                    <Button key="uninstall" size="small" danger icon={<DeleteOutlined />} disabled={!supported} loading={loadingMarketplace} onClick={() => uninstallPlugin(instanceId, plugin)}>
                      {t('marketplace.actions.uninstallPlugin')}
                    </Button>,
                  ]}
                >
                  <List.Item.Meta
                    title={(
                      <Space wrap>
                        <Text strong>{plugin.plugin}</Text>
                        <Tag>{plugin.marketplace}</Tag>
                        <Tag color={plugin.enabled ? 'green' : undefined}>{plugin.status}</Tag>
                      </Space>
                    )}
                    description={(
                      <Space direction="vertical" size={4}>
                        {plugin.message && <Text type="secondary">{plugin.message}</Text>}
                        {plugin.bundledSkills.length > 0 && <Text type="secondary">{t('marketplace.bundledSkills', { skills: plugin.bundledSkills.join(', ') })}</Text>}
                        {plugin.bundledMcpServers.length > 0 && <Text type="secondary">{t('marketplace.bundledMcpServers', { servers: plugin.bundledMcpServers.join(', ') })}</Text>}
                      </Space>
                    )}
                  />
                </List.Item>
              )}
            />
          )}
          <Form form={pluginForm} layout="vertical">
            <Form.Item
              name="marketplace"
              label={t('marketplace.form.marketplace')}
              rules={[{ required: true, whitespace: true, message: t('marketplace.form.marketplaceRequired') }]}
            >
              <Input disabled={!supported} />
            </Form.Item>
            <Form.Item
              name="plugin"
              label={t('marketplace.form.plugin')}
              rules={[{ required: true, whitespace: true, message: t('marketplace.form.pluginRequired') }]}
            >
              <Input disabled={!supported} />
            </Form.Item>
            <Button type="primary" icon={<CloudDownloadOutlined />} disabled={!supported} loading={loadingMarketplace} onClick={() => handlePluginAction('install')}>
              {t('marketplace.actions.installPlugin')}
            </Button>
          </Form>
        </Space>
      </Card>
    </Space>
  );
}
