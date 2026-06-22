import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info } from '@/utils/logger';

// Types matching the Rust backend (internally tagged via serde(tag = "type"))

export interface ToolMeta {
  auto_apply?: boolean;
  alias?: string;
  tags?: string[];
  ret_object_mapper?: Record<string, string>;
}

export interface McpServerStatus {
  name: string;
  running: boolean;
  status_message: string;
  disabled: boolean;
}

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

export interface ImportResult {
  servers_imported: number;
  inputs_imported: number;
  servers_skipped: string[];
}

interface McpServerState {
  servers: McpServerStatus[];
  loading: boolean;
  error: string | null;

  fetchServers: (instanceId: string) => Promise<void>;
  addServer: (instanceId: string, config: McpServerConfig) => Promise<void>;
  updateServer: (instanceId: string, config: McpServerConfig) => Promise<void>;
  removeServer: (instanceId: string, name: string) => Promise<void>;
  startServer: (instanceId: string, name: string) => Promise<void>;
  stopServer: (instanceId: string, name: string) => Promise<void>;
  startAll: (instanceId: string) => Promise<void>;
  stopAll: (instanceId: string) => Promise<void>;
  getServerConfig: (instanceId: string, name: string) => Promise<McpServerConfig>;
  importConfig: (instanceId: string, path: string) => Promise<ImportResult>;
  exportConfig: (instanceId: string, path: string, serverNames?: string[]) => Promise<void>;
  reset: () => void;
}

const initialState = {
  servers: [] as McpServerStatus[],
  loading: false,
  error: null as string | null,
};

export const useMcpStore = create<McpServerState>((set, get) => ({
  ...initialState,

  fetchServers: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      const servers = await invoke<McpServerStatus[]>('get_mcp_servers', { instanceId });
      set({ servers, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  addServer: async (instanceId: string, config: McpServerConfig) => {
    set({ loading: true, error: null });
    try {
      await invoke('add_mcp_server', { instanceId, config });
      info(`MCP server added: ${config.name}`);
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateServer: async (instanceId: string, config: McpServerConfig) => {
    set({ loading: true, error: null });
    try {
      await invoke('update_mcp_server', { instanceId, config });
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  removeServer: async (instanceId: string, name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('remove_mcp_server', { instanceId, name });
      info(`MCP server removed: ${name}`);
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  startServer: async (instanceId: string, name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('start_mcp_server', { instanceId, name });
      info(`MCP server started: ${name}`);
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  stopServer: async (instanceId: string, name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('stop_mcp_server', { instanceId, name });
      info(`MCP server stopped: ${name}`);
      await get().fetchServers(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
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
      set({ error: String(e), loading: false });
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
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  getServerConfig: async (instanceId: string, name: string) => {
    return await invoke<McpServerConfig>('get_mcp_server_config', { instanceId, name });
  },

  importConfig: async (instanceId: string, path: string) => {
    set({ loading: true, error: null });
    try {
      const result = await invoke<ImportResult>('import_config', { path, instanceId, format: null });
      await get().fetchServers(instanceId);
      return result;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  reset: () => set(initialState),

  exportConfig: async (instanceId: string, path: string, serverNames?: string[]) => {
    set({ loading: true, error: null });
    try {
      await invoke('export_config', { path, instanceId, serverNames: serverNames || null });
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
