import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info } from '@/utils/logger';
import {
  formatRuntimeActionError,
  isRuntimeInputCancelledError,
  type RuntimeActionError,
} from '@/utils/runtimeActionError';

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
  activation_state: McpServerActivationState;
  connection_state: McpServerConnectionState;
  /** Compatibility projection from activation_state; do not use for capability readiness. */
  running: boolean;
  status_message: string;
  disabled: boolean;
  managedBy: McpServerManagedBy;
  /** Optional only for compatibility with snapshots produced before OAuth support. */
  oauth_status?: McpOAuthStatus;
  /** Backend-owned actionability projection; machine credentials never launch a browser flow. */
  oauth_interaction?: 'none' | 'interactive' | 'machine';
}

export type McpServerActivationState = 'stopped' | 'started';

export type McpServerConnectionState =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'authorization_required'
  | 'error';

export type McpOAuthStatus =
  | { state: 'not_applicable' }
  | { state: 'unauthorized' }
  | { state: 'authorization_pending' }
  | { state: 'authorized'; scopes: string[] }
  | { state: 'reauthorization_required'; required_scope: string }
  | { state: 'error' };

export type SdkOAuthStatus = Exclude<McpOAuthStatus, { state: 'not_applicable' }>;

export type OAuthClientMode =
  | { type: 'authorizationCode'; registration: 'dynamic' }
  | {
      type: 'authorizationCode';
      registration: 'preregistered';
      clientId: string;
      clientSecretInput?: string | null;
    }
  | { type: 'authorizationCode'; registration: 'clientMetadataDocument'; url: string }
  | { type: 'clientCredentialsSecret'; clientId: string; clientSecretInput: string }
  | {
      type: 'clientCredentialsPrivateKeyJwt';
      clientId: string;
      privateKeyInput: string;
      algorithm?: string;
      tokenEndpointAudience?: string | null;
    };

export interface OAuthOptions {
  resource?: string | null;
  scopes: string[];
  clientName?: string | null;
  mode: OAuthClientMode;
}

export type HttpAuthPolicy = 'auto' | 'oauth' | 'disabled';

export type McpServerManagedBy =
  | { type: 'user' }
  | { type: 'plugin'; marketplace: string; plugin: string; pluginId?: string | null };

export interface McpBatchFailure {
  bundleId: string;
  name: string;
  error: RuntimeActionError;
}

export interface McpBatchOperationResult {
  candidate_count: number;
  actual_operation_count: number;
  unchanged_count: number;
  excluded_plugin_owned_count: number;
  failures: McpBatchFailure[];
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
  oauth?: OAuthOptions | null;
  authPolicy?: HttpAuthPolicy;
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
  serversReady: boolean;
  loading: boolean;
  error: string | null;
  activeInstanceId: string | null;
  serversRequestId: number;
  oauthEventEpoch: number;
  oauthEventEpochByBundle: Record<string, number>;
  latestOAuthStatusByBundle: Record<string, McpOAuthStatus>;

  fetchServers: (instanceId: string) => Promise<void>;
  rehydrateServers: (instanceId: string) => Promise<void>;
  startServer: (instanceId: string, bundleId: string) => Promise<void>;
  stopServer: (instanceId: string, bundleId: string) => Promise<void>;
  startAll: (instanceId: string) => Promise<McpBatchOperationResult>;
  stopAll: (instanceId: string) => Promise<McpBatchOperationResult>;
  authorizeServer: (instanceId: string, bundleId: string) => Promise<void>;
  cancelAuthorization: (instanceId: string, bundleId: string) => Promise<void>;
  clearAuthorization: (instanceId: string, bundleId: string) => Promise<void>;
  applyOAuthStatusEvent: (instanceId: string, bundleId: string, status: SdkOAuthStatus) => void;
  reset: () => void;
}

const initialState = {
  servers: [] as McpServerStatus[],
  serversReady: false,
  loading: false,
  error: null as string | null,
  activeInstanceId: null as string | null,
  serversRequestId: 0,
  oauthEventEpoch: 0,
  oauthEventEpochByBundle: {} as Record<string, number>,
  latestOAuthStatusByBundle: {} as Record<string, McpOAuthStatus>,
};

function preserveNewerOAuthEvents(
  fetched: McpServerStatus[],
  eventEpochByBundle: Record<string, number>,
  latestStatusByBundle: Record<string, McpOAuthStatus>,
  requestEpoch: number,
): McpServerStatus[] {
  return fetched.map((server) => {
    if ((eventEpochByBundle[server.bundleId] ?? 0) <= requestEpoch) return server;
    const newer = latestStatusByBundle[server.bundleId];
    return newer ? { ...server, oauth_status: newer } : server;
  });
}

export const useMcpStore = create<McpServerState>((set, get) => {
  const isActiveInstance = (instanceId: string) => get().activeInstanceId === instanceId;
  const beginInstanceAction = (instanceId: string) => {
    if (get().activeInstanceId === null) {
      set({ activeInstanceId: instanceId });
    }
    if (isActiveInstance(instanceId)) {
      set({ loading: true, error: null });
    }
  };

  return {
    ...initialState,

  fetchServers: async (instanceId: string) => {
    const requestId = get().serversRequestId + 1;
    const requestEpoch = get().oauthEventEpoch;
    const switchedInstance = get().activeInstanceId !== instanceId;
    set({
      activeInstanceId: instanceId,
      serversRequestId: requestId,
      servers: switchedInstance ? [] : get().servers,
      serversReady: false,
      loading: true,
      error: null,
      ...(switchedInstance ? {
        oauthEventEpochByBundle: {},
        latestOAuthStatusByBundle: {},
      } : {}),
    });
    try {
      const servers = await invoke<McpServerStatus[]>('get_mcp_servers', { instanceId });
      if (get().serversRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      const current = get();
      set({
        servers: preserveNewerOAuthEvents(
          servers,
          current.oauthEventEpochByBundle,
          current.latestOAuthStatusByBundle,
          requestEpoch,
        ),
        serversReady: true,
        loading: false,
      });
    } catch (e) {
      if (get().serversRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({
        serversReady: false,
        error: formatRuntimeActionError(e),
        loading: false,
      });
    }
  },

  rehydrateServers: async (instanceId: string) => {
    if (!isActiveInstance(instanceId)) return;
    const requestId = get().serversRequestId + 1;
    const requestEpoch = get().oauthEventEpoch;
    set({ serversRequestId: requestId, error: null });
    try {
      const servers = await invoke<McpServerStatus[]>('get_mcp_servers', { instanceId });
      const current = get();
      if (current.serversRequestId !== requestId || current.activeInstanceId !== instanceId) return;
      set({
        servers: preserveNewerOAuthEvents(
          servers,
          current.oauthEventEpochByBundle,
          current.latestOAuthStatusByBundle,
          requestEpoch,
        ),
        serversReady: true,
      });
    } catch (e) {
      if (get().serversRequestId === requestId && isActiveInstance(instanceId)) {
        set({ error: formatRuntimeActionError(e) });
      }
    }
  },

  startServer: async (instanceId: string, bundleId: string) => {
    beginInstanceAction(instanceId);
    try {
      await invoke('start_mcp_server', { instanceId, bundleId });
      info(`MCP server started: ${bundleId}`);
      if (isActiveInstance(instanceId)) {
        await get().fetchServers(instanceId);
      }
    } catch (e) {
      const actionError = isRuntimeInputCancelledError(e) ? null : formatRuntimeActionError(e);
      if (isActiveInstance(instanceId)) {
        await get().fetchServers(instanceId);
      }
      if (isActiveInstance(instanceId)) {
        set({ error: actionError, loading: false });
      }
      throw e;
    }
  },

  stopServer: async (instanceId: string, bundleId: string) => {
    beginInstanceAction(instanceId);
    try {
      await invoke('stop_mcp_server', { instanceId, bundleId });
      info(`MCP server stopped: ${bundleId}`);
      if (isActiveInstance(instanceId)) {
        await get().fetchServers(instanceId);
      }
    } catch (e) {
      if (isActiveInstance(instanceId)) {
        set({ error: formatRuntimeActionError(e), loading: false });
      }
      throw e;
    }
  },

  startAll: async (instanceId: string) => {
    beginInstanceAction(instanceId);
    try {
      const result = await invoke<McpBatchOperationResult>('start_all_servers', { instanceId });
      info('All MCP servers started');
      if (isActiveInstance(instanceId)) {
        await get().fetchServers(instanceId);
      }
      return result;
    } catch (e) {
      const actionError = isRuntimeInputCancelledError(e) ? null : formatRuntimeActionError(e);
      if (isActiveInstance(instanceId)) {
        await get().fetchServers(instanceId);
      }
      if (isActiveInstance(instanceId)) {
        set({ error: actionError, loading: false });
      }
      throw e;
    }
  },

  stopAll: async (instanceId: string) => {
    beginInstanceAction(instanceId);
    try {
      const result = await invoke<McpBatchOperationResult>('stop_all_servers', { instanceId });
      info('All MCP servers stopped');
      if (isActiveInstance(instanceId)) {
        await get().fetchServers(instanceId);
      }
      return result;
    } catch (e) {
      if (isActiveInstance(instanceId)) {
        set({ error: formatRuntimeActionError(e), loading: false });
      }
      throw e;
    }
  },

  authorizeServer: async (instanceId: string, bundleId: string) => {
    beginInstanceAction(instanceId);
    try {
      await invoke('authorize_mcp_server', { instanceId, bundleId });
      if (isActiveInstance(instanceId)) set({ loading: false });
    } catch (e) {
      if (isActiveInstance(instanceId)) {
        set({ error: formatRuntimeActionError(e), loading: false });
      }
      throw e;
    }
  },

  cancelAuthorization: async (instanceId: string, bundleId: string) => {
    beginInstanceAction(instanceId);
    try {
      await invoke('cancel_mcp_authorization', { instanceId, bundleId });
      if (isActiveInstance(instanceId)) set({ loading: false });
    } catch (e) {
      if (isActiveInstance(instanceId)) {
        set({ error: formatRuntimeActionError(e), loading: false });
      }
      throw e;
    }
  },

  clearAuthorization: async (instanceId: string, bundleId: string) => {
    beginInstanceAction(instanceId);
    try {
      await invoke('clear_mcp_authorization', { instanceId, bundleId });
      if (isActiveInstance(instanceId)) set({ loading: false });
    } catch (e) {
      if (isActiveInstance(instanceId)) {
        set({ error: formatRuntimeActionError(e), loading: false });
      }
      throw e;
    }
  },

  applyOAuthStatusEvent: (instanceId, bundleId, status) => {
    if (!isActiveInstance(instanceId)) return;
    const oauthStatus: McpOAuthStatus = status;
    set((current) => {
      const oauthEventEpoch = current.oauthEventEpoch + 1;
      return {
        oauthEventEpoch,
        oauthEventEpochByBundle: {
          ...current.oauthEventEpochByBundle,
          [bundleId]: oauthEventEpoch,
        },
        latestOAuthStatusByBundle: {
          ...current.latestOAuthStatusByBundle,
          [bundleId]: oauthStatus,
        },
        servers: current.servers.map((server) => (
          server.bundleId === bundleId ? { ...server, oauth_status: oauthStatus } : server
        )),
      };
    });
  },

  reset: () => set(initialState),
  };
});
