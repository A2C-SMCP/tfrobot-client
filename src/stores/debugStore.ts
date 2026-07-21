import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface ToolInfo {
  name: string;
  displayName: string;
  description: string;
  inputSchema: Record<string, unknown>;
  server: string;
  bundleId?: string;
  tags?: string[];
}

export interface ContentItem {
  type: 'text' | 'image' | 'resource';
  text?: string;
  data?: string;
  mime_type?: string;
  uri?: string;
}

export interface CallToolResult {
  content: ContentItem[];
  isError?: boolean;
  _meta?: Record<string, unknown>;
}

export interface ToolCallResponse {
  success: boolean;
  result?: CallToolResult;
  error?: string;
  duration_ms: number;
}

export interface ToolCallHistoryRecord {
  timestamp: string;
  req_id: string;
  computer_instance_id: string;
  server: string;
  tool: string;
  parameters: Record<string, unknown>;
  timeout?: number;
  success: boolean;
  error?: string;
}

export interface DebugResourceInfo {
  server: string;
  uri: string;
  name: string;
  description?: string;
  mime_type?: string;
}

export interface DebugResourcesResponse {
  resources: DebugResourceInfo[];
  next_cursor?: string;
}

interface DebugState {
  tools: ToolInfo[];
  toolsLoading: boolean;
  selectedTool: ToolInfo | null;
  lastCallResult: ToolCallResponse | null;
  calling: boolean;
  resources: DebugResourceInfo[];
  resourcesLoading: boolean;
  resourcesNextCursor: string | null;
  history: ToolCallHistoryRecord[];
  historyLoading: boolean;
  error: string | null;
  activeInstanceId: string | null;
  toolsRequestId: number;
  resourcesRequestId: number;
  historyRequestId: number;
  executionRequestId: number;

  fetchTools: (instanceId: string) => Promise<void>;
  fetchResources: (instanceId: string, bundleId: string, cursor?: string | null) => Promise<void>;
  selectTool: (tool: ToolInfo | null) => void;
  executeTool: (instanceId: string, toolName: string, params: Record<string, unknown>, timeout?: number) => Promise<ToolCallResponse>;
  fetchHistory: (instanceId: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  tools: [] as ToolInfo[],
  toolsLoading: false,
  selectedTool: null as ToolInfo | null,
  lastCallResult: null as ToolCallResponse | null,
  calling: false,
  resources: [] as DebugResourceInfo[],
  resourcesLoading: false,
  resourcesNextCursor: null as string | null,
  history: [] as ToolCallHistoryRecord[],
  historyLoading: false,
  error: null as string | null,
  activeInstanceId: null as string | null,
  toolsRequestId: 0,
  resourcesRequestId: 0,
  historyRequestId: 0,
  executionRequestId: 0,
};

export const useDebugStore = create<DebugState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchTools: async (instanceId: string) => {
    const requestId = get().toolsRequestId + 1;
    const switchingInstance = get().activeInstanceId !== instanceId;
    set({
      activeInstanceId: instanceId,
      toolsRequestId: requestId,
      executionRequestId: switchingInstance ? get().executionRequestId + 1 : get().executionRequestId,
      tools: [],
      selectedTool: null,
      lastCallResult: null,
      calling: switchingInstance ? false : get().calling,
      toolsLoading: true,
      error: null,
    });
    try {
      const tools = await invoke<ToolInfo[]>('get_available_tools', { instanceId });
      if (get().toolsRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ tools, toolsLoading: false });
    } catch (e) {
      if (get().toolsRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: String(e), toolsLoading: false });
    }
  },

  fetchResources: async (instanceId, bundleId, cursor = null) => {
    const requestId = get().resourcesRequestId + 1;
    const switchingInstance = get().activeInstanceId !== instanceId;
    set({
      activeInstanceId: instanceId,
      resourcesRequestId: requestId,
      executionRequestId: switchingInstance ? get().executionRequestId + 1 : get().executionRequestId,
      resources: cursor ? get().resources : [],
      resourcesNextCursor: cursor ? get().resourcesNextCursor : null,
      lastCallResult: switchingInstance ? null : get().lastCallResult,
      calling: switchingInstance ? false : get().calling,
      resourcesLoading: true,
      error: null,
    });
    try {
      const response = await invoke<DebugResourcesResponse>('get_debug_resources', {
        instanceId,
        bundleId,
        cursor: cursor ?? null,
      });
      if (get().resourcesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set((state) => ({
        resources: cursor ? [...state.resources, ...response.resources] : response.resources,
        resourcesNextCursor: response.next_cursor ?? null,
        resourcesLoading: false,
      }));
    } catch (e) {
      if (get().resourcesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: String(e), resourcesLoading: false, resourcesNextCursor: null });
    }
  },

  selectTool: (tool) => set({ selectedTool: tool, lastCallResult: null }),

  executeTool: async (instanceId, toolName, params, timeout) => {
    const requestId = get().executionRequestId + 1;
    set({
      activeInstanceId: instanceId,
      executionRequestId: requestId,
      calling: true,
      lastCallResult: null,
      error: null,
    });
    try {
      const result = await invoke<ToolCallResponse>('execute_tool', {
        instanceId,
        toolName,
        params,
        timeout: timeout ?? null,
      });
      if (get().executionRequestId === requestId && get().activeInstanceId === instanceId) {
        set({ lastCallResult: result, calling: false });
      }
      if (get().executionRequestId === requestId && get().activeInstanceId === instanceId) {
        get().fetchHistory(instanceId);
      }
      return result;
    } catch (e) {
      const errResult: ToolCallResponse = {
        success: false,
        error: String(e),
        duration_ms: 0,
      };
      if (get().executionRequestId === requestId && get().activeInstanceId === instanceId) {
        set({ lastCallResult: errResult, calling: false });
      }
      return errResult;
    }
  },

  fetchHistory: async (instanceId) => {
    const requestId = get().historyRequestId + 1;
    const switchingInstance = get().activeInstanceId !== instanceId;
    set({
      activeInstanceId: instanceId,
      historyRequestId: requestId,
      executionRequestId: switchingInstance ? get().executionRequestId + 1 : get().executionRequestId,
      history: [],
      lastCallResult: switchingInstance ? null : get().lastCallResult,
      calling: switchingInstance ? false : get().calling,
      historyLoading: true,
      error: null,
    });
    try {
      const history = await invoke<ToolCallHistoryRecord[]>('get_tool_history', { instanceId });
      if (get().historyRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ history, historyLoading: false });
    } catch (e) {
      if (get().historyRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: String(e), historyLoading: false });
    }
  },
}));
