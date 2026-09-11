import { usePageActive } from '@/components/Navigation/pageActivityState';
import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { useEffect, useMemo, useRef, useState } from 'react';
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
  const pageActive = usePageActive();
  const { servers, loading: serversLoading, activeInstanceId, fetchServers } = useMcpStore();
  const {
    resources: storedResources,
    resourceQuery,
    resourcesLoading,
    resourcesNextCursor,
    error,
    fetchResources,
  } = useDebugStore();
  const [selectedServer, setSelectedServer] = useNavigationState<{
    instanceId: string;
    bundleId: string;
    name: string;
  } | null>('debug.resourceServer', null);
  const [serversReady, setServersReady] = useState(false);
  const selectedBundleId = selectedServer?.instanceId === instanceId ? selectedServer.bundleId : undefined;
  const serversBelongToInstance = activeInstanceId === instanceId;
  const [pageCounts, setPageCounts] = useNavigationState<Record<string, number>>('debug.resourcePages', {});
  const pageCount = selectedBundleId ? pageCounts[selectedBundleId] ?? 1 : 1;
  const [refreshRevision, setRefreshRevision] = useState(0);
  const forceRefresh = useRef(false);
  const queryKey = JSON.stringify([instanceId, selectedBundleId]);
  const resultKey = JSON.stringify([resourceQuery?.instanceId, resourceQuery?.bundleId]);
  const resources = resultKey === queryKey ? storedResources : [];

  const runningServers = useMemo(
    () => (
      serversBelongToInstance
        ? servers.filter((server) => server.connection_state === 'connected' && !server.disabled)
        : []
    ),
    [servers, serversBelongToInstance],
  );

  useEffect(() => {
    if (!pageActive) return;
    let active = true;
    setServersReady(false);
    fetchServers(instanceId).finally(() => {
      if (active) {
        setServersReady(true);
      }
    });
    return () => {
      active = false;
    };
  }, [fetchServers, instanceId, pageActive]);

  useEffect(() => {
    if (!pageActive || !serversReady || serversLoading || !serversBelongToInstance) {
      return;
    }
    const ids = new Set(servers.map((server) => server.bundleId));
    if (Object.keys(pageCounts).some((id) => !ids.has(id))) {
      setPageCounts(Object.fromEntries(Object.entries(pageCounts).filter(([id]) => ids.has(id))));
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
  }, [instanceId, runningServers, selectedBundleId, serversReady, serversLoading, serversBelongToInstance, pageActive, setSelectedServer, servers, pageCounts, setPageCounts]);

  const selectedAvailable = runningServers.some((server) => server.bundleId === selectedBundleId);
  useEffect(() => {
    if (!pageActive || !serversReady || serversLoading || !selectedBundleId || !selectedAvailable) return;
    let cancelled = false;
    const ownsResult = () => {
      const query = useDebugStore.getState().resourceQuery;
      return query?.instanceId === instanceId && query.bundleId === selectedBundleId;
    };
    const restoreRange = async () => {
      let pages = !forceRefresh.current && ownsResult() ? useDebugStore.getState().resourcePagesLoaded ?? 0 : 0;
      if (pages === 0) {
        await fetchResources(instanceId, selectedBundleId);
        if (cancelled || !ownsResult() || useDebugStore.getState().error) return;
        pages = 1;
        forceRefresh.current = false;
      }
      while (!cancelled && pages < pageCount) {
        const cursor = useDebugStore.getState().resourcesNextCursor;
        if (!cursor) break;
        await fetchResources(instanceId, selectedBundleId, cursor);
        if (cancelled || !ownsResult() || useDebugStore.getState().error) return;
        pages += 1;
        forceRefresh.current = false;
      }
    };
    void restoreRange();
    return () => { cancelled = true; };
  }, [fetchResources, instanceId, pageActive, pageCount, queryKey, selectedAvailable, selectedBundleId, serversLoading, serversReady, refreshRevision]);

  const handleRefresh = () => {
    forceRefresh.current = true;
    setRefreshRevision((revision) => revision + 1);
    void fetchServers(instanceId);
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

      {(serversLoading || resourcesLoading) && resources.length === 0 ? (
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
              onClick={() => selectedBundleId && setPageCounts((counts) => ({ ...counts, [selectedBundleId]: (counts[selectedBundleId] ?? 1) + 1 }))}
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
