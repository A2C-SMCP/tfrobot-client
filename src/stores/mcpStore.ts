import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

// Types matching the Rust backend
export interface McpServerStatus {
  name: string;
  running: boolean;
  status_message: string;
  disabled: boolean;
}

export interface StdioServerConfig {
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd?: string;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, unknown>;
}

export interface HttpServerConfig {
  name: string;
  url: string;
  headers: Record<string, string>;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, unknown>;
}

export interface SseServerConfig {
  name: string;
  url: string;
  headers: Record<string, string>;
  disabled: boolean;
  forbidden_tools: string[];
  tool_meta: Record<string, unknown>;
}

export type McpServerConfig =
  | { Stdio: StdioServerConfig }
  | { Http: HttpServerConfig }
  | { Sse: SseServerConfig };

// Helper to get server name from config
export function getConfigName(config: McpServerConfig): string {
  if ('Stdio' in config) return config.Stdio.name;
  if ('Http' in config) return config.Http.name;
  if ('Sse' in config) return config.Sse.name;
  return '';
}

// Helper to get config type
export function getConfigType(config: McpServerConfig): 'stdio' | 'http' | 'sse' {
  if ('Stdio' in config) return 'stdio';
  if ('Http' in config) return 'http';
  if ('Sse' in config) return 'sse';
  return 'stdio';
}

interface McpServerState {
  servers: McpServerStatus[];
  loading: boolean;
  error: string | null;

  // Actions
  fetchServers: () => Promise<void>;
  addServer: (config: McpServerConfig) => Promise<void>;
  updateServer: (config: McpServerConfig) => Promise<void>;
  removeServer: (name: string) => Promise<void>;
  startServer: (name: string) => Promise<void>;
  stopServer: (name: string) => Promise<void>;
  startAll: () => Promise<void>;
  stopAll: () => Promise<void>;
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
}));
