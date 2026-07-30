import { useEffect, useMemo, useState } from 'react';
import { Button, Select, Space, List, Tag, Typography, Empty, Spin, Divider } from 'antd';
import { Input } from '@/components/common/Input';
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useDebugStore, type ToolInfo } from '@/stores/debugStore';
import { ToolCallTest } from './ToolCallTest';

const { Text, Paragraph } = Typography;
const UNKNOWN_SERVER = 'unknown';

interface ToolBrowserProps {
  instanceId: string;
}

export function ToolBrowser({ instanceId }: ToolBrowserProps) {
  const { t } = useTranslation();
  const { tools, toolsLoading, selectedTool, fetchTools, selectTool } = useDebugStore();
  const [search, setSearch] = useState('');
  const [serverFilter, setServerFilter] = useState<string | undefined>();

  useEffect(() => {
    selectTool(null);
    fetchTools(instanceId);
  }, [fetchTools, instanceId, selectTool]);

  const servers = useMemo(
    () => [...new Set(tools.map((t) => t.server).filter((server) => server && server !== UNKNOWN_SERVER))],
    [tools],
  );

  useEffect(() => {
    if (serverFilter && !servers.includes(serverFilter)) {
      setServerFilter(undefined);
    }
  }, [serverFilter, servers]);

  const filtered = tools.filter((tool) => {
    if (search && !tool.displayName.toLowerCase().includes(search.toLowerCase()) && !tool.description.toLowerCase().includes(search.toLowerCase())) {
      return false;
    }
    if (serverFilter && tool.server !== serverFilter) return false;
    return true;
  });

  return (
    <div style={{ display: 'flex', gap: 16, height: 'calc(100vh - 220px)' }}>
      {/* Left: Tool List */}
      <div style={{ width: 320, flexShrink: 0, display: 'flex', flexDirection: 'column' }}>
        <Space style={{ marginBottom: 8, width: '100%' }} direction="vertical">
          <Space.Compact style={{ width: '100%' }}>
            <Input
              prefix={<SearchOutlined />}
              placeholder={t('debug.searchTools')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              allowClear
            />
            <Button icon={<ReloadOutlined />} onClick={() => fetchTools(instanceId)} loading={toolsLoading} />
          </Space.Compact>
          {servers.length > 0 && (
            <Select
              style={{ width: '100%' }}
              placeholder={t('debug.allServers')}
              value={serverFilter}
              onChange={setServerFilter}
              allowClear
              options={servers.map((s) => ({ label: s, value: s }))}
            />
          )}
        </Space>

        <div style={{ flex: 1, overflow: 'auto' }}>
          {toolsLoading ? (
            <Spin style={{ display: 'block', marginTop: 40 }} />
          ) : filtered.length === 0 ? (
            <Empty description={t('debug.noTools')} />
          ) : (
            <List
              size="small"
              dataSource={filtered}
              renderItem={(tool) => (
                <List.Item
                  onClick={() => selectTool(tool)}
                  style={{
                    cursor: 'pointer',
                    background: selectedTool?.name === tool.name ? '#e6f4ff' : undefined,
                    padding: '8px 12px',
                  }}
                >
                  <List.Item.Meta
                    title={
                      <Space>
                        <Text strong>{tool.displayName}</Text>
                        {tool.server !== UNKNOWN_SERVER && <Tag>{tool.server}</Tag>}
                      </Space>
                    }
                    description={
                      <Text type="secondary" ellipsis>
                        {tool.description}
                      </Text>
                    }
                  />
                </List.Item>
              )}
            />
          )}
        </div>
      </div>

      {/* Right: Tool Detail + Call Test */}
      <div style={{ flex: 1, overflow: 'auto' }}>
        {selectedTool ? (
          <ToolDetail instanceId={instanceId} tool={selectedTool} />
        ) : (
          <Empty description={t('debug.selectTool')} style={{ marginTop: 80 }} />
        )}
      </div>
    </div>
  );
}

function ToolDetail({ instanceId, tool }: { instanceId: string; tool: ToolInfo }) {
  const { t } = useTranslation();

  return (
    <div>
      <Typography.Title level={5}>{tool.displayName}</Typography.Title>
      <Space style={{ marginBottom: 8 }}>
        {tool.server !== UNKNOWN_SERVER && <Tag color="blue">{tool.server}</Tag>}
        {tool.tags?.map((tag) => <Tag key={tag}>{tag}</Tag>)}
      </Space>
      <Paragraph>{tool.description}</Paragraph>

      <Divider orientation="left">{t('debug.inputSchema')}</Divider>
      <pre style={{ background: '#f5f5f5', padding: 12, borderRadius: 6, fontSize: 12, maxHeight: 200, overflow: 'auto' }}>
        {JSON.stringify(tool.inputSchema, null, 2)}
      </pre>

      <Divider orientation="left">{t('debug.callTest')}</Divider>
      <ToolCallTest instanceId={instanceId} tool={tool} />
    </div>
  );
}
