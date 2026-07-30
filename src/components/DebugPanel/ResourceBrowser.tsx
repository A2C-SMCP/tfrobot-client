import { useEffect, useMemo, useState } from 'react';
import { Alert, Button, Empty, List, Select, Space, Spin, Tag, Typography } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useDebugStore } from '@/stores/debugStore';
import { useMcpStore } from '@/stores/mcpStore';

const { Text, Paragraph } = Typography;

interface ResourceBrowserProps {
  instanceId: string;
}

export function ResourceBrowser({ instanceId }: ResourceBrowserProps) {
  const { t } = useTranslation();
  const { servers, loading: serversLoading, activeInstanceId, fetchServers } = useMcpStore();
  const {
    resources,
    resourcesLoading,
    resourcesNextCursor,
    error,
    fetchResources,
  } = useDebugStore();
  const [selectedServer, setSelectedServer] = useState<{
    instanceId: string;
    bundleId: string;
    name: string;
  } | null>(null);
  const [serversReady, setServersReady] = useState(false);
  const selectedBundleId = selectedServer?.instanceId === instanceId ? selectedServer.bundleId : undefined;
  const serversBelongToInstance = activeInstanceId === instanceId;

  const runningServers = useMemo(
    () => (
      serversBelongToInstance
        ? servers.filter((server) => server.running && !server.disabled)
        : []
    ),
    [servers, serversBelongToInstance],
  );

  useEffect(() => {
    let active = true;
    setServersReady(false);
    setSelectedServer(null);
    fetchServers(instanceId).finally(() => {
      if (active) {
        setServersReady(true);
      }
    });
    return () => {
      active = false;
    };
  }, [fetchServers, instanceId]);

  useEffect(() => {
    if (!serversReady) {
      return;
    }
    if (selectedBundleId && !runningServers.some((server) => server.bundleId === selectedBundleId)) {
      setSelectedServer(null);
      return;
    }
    if (!selectedBundleId && runningServers.length > 0) {
      setSelectedServer({
        instanceId,
        bundleId: runningServers[0].bundleId,
        name: runningServers[0].name,
      });
    }
  }, [instanceId, runningServers, selectedBundleId, serversReady]);

  useEffect(() => {
    if (
      serversReady &&
      selectedBundleId &&
      runningServers.some((server) => server.bundleId === selectedBundleId)
    ) {
      fetchResources(instanceId, selectedBundleId);
    }
  }, [fetchResources, instanceId, runningServers, selectedBundleId, serversReady]);

  const handleRefresh = () => {
    setServersReady(false);
    fetchServers(instanceId).finally(() => setServersReady(true));
    if (
      selectedBundleId &&
      runningServers.some((server) => server.bundleId === selectedBundleId)
    ) {
      fetchResources(instanceId, selectedBundleId);
    }
  };

  return (
    <div>
      <Space direction="vertical" style={{ width: '100%', marginBottom: 16 }}>
        <Space wrap>
          <Select
            placeholder={t('debug.selectResourceServer')}
            style={{ minWidth: 220 }}
            value={selectedBundleId}
            loading={serversLoading || !serversReady}
            onChange={(bundleId) => {
              const server = runningServers.find((candidate) => candidate.bundleId === bundleId);
              setSelectedServer(server ? { instanceId, bundleId, name: server.name } : null);
            }}
            options={runningServers.map((server) => ({
              label: server.name,
              value: server.bundleId,
            }))}
          />
          <Button
            icon={<ReloadOutlined />}
            onClick={handleRefresh}
            loading={resourcesLoading || serversLoading}
          >
            {t('common.refresh')}
          </Button>
          <Text type="secondary">Computer: {instanceId}</Text>
        </Space>
        {error && (
          <Alert
            type="error"
            message={t('common.error')}
            description={error}
            showIcon
          />
        )}
      </Space>

      {serversLoading || (resourcesLoading && resources.length === 0) ? (
        <Spin style={{ display: 'block', marginTop: 40 }} />
      ) : runningServers.length === 0 ? (
        <Empty description={t('debug.noRunningServers')} />
      ) : resources.length === 0 ? (
        <Empty description={t('debug.noResources')} />
      ) : (
        <Space direction="vertical" style={{ width: '100%' }}>
          <List
            size="small"
            dataSource={resources}
            renderItem={(resource) => (
              <List.Item>
                <List.Item.Meta
                  title={
                    <Space wrap>
                      <Text strong>{resource.name}</Text>
                      <Tag>{resource.server}</Tag>
                      {resource.mime_type && <Tag color="blue">{resource.mime_type}</Tag>}
                    </Space>
                  }
                  description={
                    <Space direction="vertical" size={2} style={{ width: '100%' }}>
                      <Text code copyable>
                        {resource.uri}
                      </Text>
                      {resource.description && (
                        <Paragraph type="secondary" style={{ margin: 0 }}>
                          {resource.description}
                        </Paragraph>
                      )}
                    </Space>
                  }
                />
              </List.Item>
            )}
          />
          {resourcesNextCursor && (
            <Button
              onClick={() => selectedBundleId && fetchResources(
                instanceId,
                selectedBundleId,
                resourcesNextCursor,
              )}
              loading={resourcesLoading}
            >
              {t('debug.loadMoreResources')}
            </Button>
          )}
        </Space>
      )}
    </div>
  );
}
