import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info } from '@/utils/logger';
import { formatRuntimeActionError } from '@/utils/runtimeActionError';

// Types matching the Rust backend (internally tagged via serde(tag = "type"))

export interface ToolMeta {
  auto_apply?: boolean;
  alias?: string;
  tags?: string[];
  ret_object_mapper?: Record<string, string>;
}

export interface McpServerStatus {
  bundleId: string;
  name: string;
  running: boolean;
  status_message: string;
  disabled: boolean;
  managedBy: McpServerManagedBy;
}

export type McpServerManagedBy =
  | { type: 'user' }
  | { type: 'plugin'; marketplace: string; plugin: string; pluginId?: string | null };

// server_parameters sub-types
export interface StdioServerParameters {
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd?: string | null;
}

export interface HttpServerParameters {
  url: string;
  headers: Record<string, string>;
}

export interface SseServerParameters {
  url: string;
  headers: Record<string, string>;
}

// Internally tagged discriminated union configs
export interface StdioServerConfig {
  type: 'Stdio';
  bundle_id?: string;
  name: string;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, ToolMeta>;
  default_tool_meta?: ToolMeta | null;
  vrl?: string | null;
  server_parameters: StdioServerParameters;
}

export interface HttpServerConfig {
  type: 'Http';
  bundle_id?: string;
  name: string;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, ToolMeta>;
  default_tool_meta?: ToolMeta | null;
  vrl?: string | null;
  server_parameters: HttpServerParameters;
}

export interface SseServerConfig {
  type: 'Sse';
  bundle_id?: string;
  name: string;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, ToolMeta>;
  default_tool_meta?: ToolMeta | null;
  vrl?: string | null;
  server_parameters: SseServerParameters;
}

export type McpServerConfig = StdioServerConfig | HttpServerConfig | SseServerConfig;

// Helper to get server name from config
export function getConfigName(config: McpServerConfig): string {
  return config.name;
}

// Helper to get config type
export function getConfigType(config: McpServerConfig): 'stdio' | 'http' | 'sse' {
  return config.type.toLowerCase() as 'stdio' | 'http' | 'sse';
}

interface McpServerState {
  servers: McpServerStatus[];
  loading: boolean;
  error: string | null;
  activeInstanceId: string | null;
  serversRequestId: number;

  fetchServers: (instanceId: string) => Promise<void>;
  startServer: (instanceId: string, bundleId: string) => Promise<void>;
  stopServer: (instanceId: string, bundleId: string) => Promise<void>;
  startAll: (instanceId: string) => Promise<void>;
  stopAll: (instanceId: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  servers: [] as McpServerStatus[],
  loading: false,
  error: null as string | null,
  activeInstanceId: null as string | null,
  serversRequestId: 0,
};

export const useMcpStore = create<McpServerState>((set, get) => ({
  ...initialState,

  fetchServers: async (instanceId: string) => {
    const requestId = get().serversRequestId + 1;
    set({
      activeInstanceId: instanceId,
      serversRequestId: requestId,
      servers: [],
      loading: true,
      error: null,
    });
    try {
      const servers = await invoke<McpServerStatus[]>('get_mcp_servers', { instanceId });
      if (get().serversRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ servers, loading: false });
    } catch (e) {
      if (get().serversRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: formatRuntimeActionError(e), loading: false });
    }
  },

  startServer: async (instanceId: string, bundleId: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('start_mcp_server', { instanceId, bundleId });
      info(`MCP server started: ${bundleId}`);
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  stopServer: async (instanceId: string, bundleId: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('stop_mcp_server', { instanceId, bundleId });
      info(`MCP server stopped: ${bundleId}`);
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  startAll: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('start_all_servers', { instanceId });
      info('All MCP servers started');
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  stopAll: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('stop_all_servers', { instanceId });
      info('All MCP servers stopped');
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: formatRuntimeActionError(e), loading: false });
      throw e;
    }
  },

  reset: () => set(initialState),
}));
