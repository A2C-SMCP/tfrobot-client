import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

// Types matching the Rust backend (internally tagged via serde(tag = "type"))

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
  tool_meta: Record<string, unknown>;
  default_tool_meta?: unknown | null;
  vrl?: string | null;
  server_parameters: StdioServerParameters;
}

export interface HttpServerConfig {
  type: 'Http';
  name: string;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, unknown>;
  default_tool_meta?: unknown | null;
  vrl?: string | null;
  server_parameters: HttpServerParameters;
}

export interface SseServerConfig {
  type: 'Sse';
  name: string;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, unknown>;
  default_tool_meta?: unknown | null;
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

  fetchServers: () => Promise<void>;
  addServer: (config: McpServerConfig) => Promise<void>;
  updateServer: (config: McpServerConfig) => Promise<void>;
  removeServer: (name: string) => Promise<void>;
  startServer: (name: string) => Promise<void>;
  stopServer: (name: string) => Promise<void>;
  startAll: () => Promise<void>;
  stopAll: () => Promise<void>;
  importConfig: (path: string) => Promise<ImportResult>;
  exportConfig: (path: string, serverNames?: string[]) => Promise<void>;
}

export const useMcpStore = create<McpServerState>((set, get) => ({
  servers: [],
  loading: false,
  error: null,

  fetchServers: async () => {
    set({ loading: true, error: null });
    try {
      const servers = await invoke<McpServerStatus[]>('get_mcp_servers');
      set({ servers, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  addServer: async (config: McpServerConfig) => {
    set({ loading: true, error: null });
    try {
      await invoke('add_mcp_server', { config });
      await get().fetchServers();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateServer: async (config: McpServerConfig) => {
    set({ loading: true, error: null });
    try {
      await invoke('update_mcp_server', { config });
      await get().fetchServers();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  removeServer: async (name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('remove_mcp_server', { name });
      await get().fetchServers();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  startServer: async (name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('start_mcp_server', { name });
      await get().fetchServers();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  stopServer: async (name: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('stop_mcp_server', { name });
      await get().fetchServers();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  startAll: async () => {
    set({ loading: true, error: null });
    try {
      await invoke('start_all_servers');
      await get().fetchServers();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  stopAll: async () => {
    set({ loading: true, error: null });
    try {
      await invoke('stop_all_servers');
      await get().fetchServers();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  importConfig: async (path: string) => {
    set({ loading: true, error: null });
    try {
      const result = await invoke<ImportResult>('import_config', { path, format: null });
      await get().fetchServers();
      return result;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  exportConfig: async (path: string, serverNames?: string[]) => {
    set({ loading: true, error: null });
    try {
      await invoke('export_config', { path, serverNames: serverNames || null });
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
