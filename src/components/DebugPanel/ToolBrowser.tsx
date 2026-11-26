import { useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { usePageActive } from '@/components/Navigation/pageActivityState';
import { useEffect, useMemo } from 'react';
import { Alert, Button, Space, List, Tag, Typography, Empty, Spin, Divider } from 'antd';
import { Input } from '@/components/common/Input';
import { McpSourceFilter } from '@/components/McpConfig/McpSourceFilter';
import { ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import { useDebugStore, type ToolInfo } from '@/stores/debugStore';
import { ToolCallTest } from './ToolCallTest';
import styles from './ToolBrowser.module.css';

const { Text, Paragraph } = Typography;
const UNKNOWN_SERVER = 'unknown';
const EMPTY_TOOLS: ToolInfo[] = [];

interface ToolBrowserProps {
  instanceId: string;
  onViewLog?: (correlationId: string) => void;
}

const sourceId = (tool: ToolInfo) => tool.bundleId ?? tool.server;

export function ToolBrowser({ instanceId, onViewLog }: ToolBrowserProps) {
  const { t } = useTranslation();
  const active = usePageActive();
  const state = useDebugStore();
  const { fetchTools, selectTool } = state;
  const ownsTools = state.activeInstanceId === instanceId && state.toolsLoadedInstanceId === instanceId;
  const tools = ownsTools ? state.tools : EMPTY_TOOLS;
  const toolsLoading = state.toolsLoading || !ownsTools;
  const authoritative = ownsTools && !state.toolsLoading && !state.error;
  const [selectedName, setSelectedName] = useNavigationState<string | null>('debug.selectedTool', null);
  const [search, setSearch] = useNavigationState('debug.search', '');
  const [serverFilter, setServerFilter] = useNavigationState<string[] | undefined>('debug.servers', undefined);

  useEffect(() => {
    if (active) void fetchTools(instanceId);
  }, [active, fetchTools, instanceId]);

  const servers = useMemo(
    () => [...new Map(tools.map((tool) => [sourceId(tool), tool.server === UNKNOWN_SERVER ? t('debug.unknownServer') : tool.server]))]
      .map(([value, label]) => ({ value, label })),
    [t, tools],
  );
  const term = search.trim().toLowerCase();
  const filtered = tools.filter((tool) =>
    (serverFilter === undefined || serverFilter.includes(sourceId(tool))) &&
    [tool.name, tool.displayName, tool.description, tool.server].some((value) => value.toLowerCase().includes(term)));
  const nextSelection = filtered.find((tool) => tool.name === selectedName) ?? filtered[0] ?? null;
  useEffect(() => {
    if (active && authoritative && nextSelection?.name !== state.selectedTool?.name) {
      selectTool(nextSelection);
    }
  }, [active, authoritative, nextSelection, selectTool, state.selectedTool?.name]);
  const selectedTool = nextSelection?.name === state.selectedTool?.name ? nextSelection : null;

  return (
    <div className={styles.browser}>
      {/* Left: Tool List */}
      <div className={styles.listPane}>
        <Space style={{ marginBottom: 8, width: '100%' }} direction="vertical">
          <Space.Compact style={{ width: '100%' }}>
            <Input
              prefix={<SearchOutlined />}
              placeholder={t('debug.searchTools')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              allowClear
            />
            <Button aria-label={t('common.refresh')} icon={<ReloadOutlined />} onClick={() => fetchTools(instanceId)}
              loading={state.activeInstanceId === instanceId && state.toolsLoading} />
          </Space.Compact>
          {servers.length > 0 && (
            <McpSourceFilter options={servers} selection={serverFilter} onChange={setServerFilter} />
          )}
        </Space>

        <div data-navigation-scroll="tools-list" style={{ flex: 1, overflow: 'auto' }}>
          {state.activeInstanceId === instanceId && state.error ? (
            <Alert type="error" message={state.error} />
          ) : toolsLoading && tools.length === 0 ? (
            <Spin style={{ display: 'block', marginTop: 40 }} />
          ) : filtered.length === 0 ? (
            <Empty description={t('debug.noTools')} />
          ) : (
            <List
              size="small"
              dataSource={filtered}
              renderItem={(tool) => (
                <List.Item className={styles.toolRow}>
                  <button type="button" className={styles.tool}
                    aria-pressed={selectedTool?.name === tool.name}
                    onClick={() => setSelectedName(tool.name)}>
                    <Text strong className={styles.toolName}>{tool.displayName}</Text>
                    {tool.server !== UNKNOWN_SERVER && <Text type="secondary">{tool.server}</Text>}
                  </button>
                </List.Item>
              )}
            />
          )}
        </div>
      </div>

      {/* Right: Tool Detail + Call Test */}
      <div data-navigation-scroll="tools-detail" className={styles.detail}>
        {selectedTool ? (
          <ToolDetail instanceId={instanceId} tool={selectedTool} onViewLog={onViewLog} />
        ) : (
          <Empty description={t('debug.selectTool')} style={{ marginTop: 80 }} />
        )}
      </div>
    </div>
  );
}

function ToolDetail({ instanceId, tool, onViewLog }: { instanceId: string; tool: ToolInfo; onViewLog?: ToolBrowserProps['onViewLog'] }) {
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
      <pre data-navigation-scroll="tool-schema" className={styles.schema}>
        {JSON.stringify(tool.inputSchema, null, 2)}
      </pre>

      <Divider orientation="left">{t('debug.callTest')}</Divider>
      <ToolCallTest key={tool.name} instanceId={instanceId} tool={tool} onViewLog={onViewLog} />
    </div>
  );
}
