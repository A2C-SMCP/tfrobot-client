import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface ToolInfo {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  server: string;
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
  is_error: boolean;
  meta?: Record<string, unknown>;
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
  server: string;
  tool: string;
  parameters: Record<string, unknown>;
  timeout?: number;
  success: boolean;
  error?: string;
}

interface DebugState {
  tools: ToolInfo[];
  toolsLoading: boolean;
  selectedTool: ToolInfo | null;
  lastCallResult: ToolCallResponse | null;
  calling: boolean;
  history: ToolCallHistoryRecord[];
  historyLoading: boolean;
  error: string | null;

  fetchTools: () => Promise<void>;
  selectTool: (tool: ToolInfo | null) => void;
  executeTool: (toolName: string, params: Record<string, unknown>, timeout?: number) => Promise<ToolCallResponse>;
  fetchHistory: () => Promise<void>;
  reset: () => void;
}

const initialState = {
  tools: [] as ToolInfo[],
  toolsLoading: false,
  selectedTool: null as ToolInfo | null,
  lastCallResult: null as ToolCallResponse | null,
  calling: false,
  history: [] as ToolCallHistoryRecord[],
  historyLoading: false,
  error: null as string | null,
};

export const useDebugStore = create<DebugState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchTools: async () => {
    set({ toolsLoading: true, error: null });
    try {
      const tools = await invoke<ToolInfo[]>('get_available_tools');
      set({ tools, toolsLoading: false });
    } catch (e) {
      set({ error: String(e), toolsLoading: false });
    }
  },

  selectTool: (tool) => set({ selectedTool: tool, lastCallResult: null }),

  executeTool: async (toolName, params, timeout) => {
    set({ calling: true, error: null });
    try {
      const result = await invoke<ToolCallResponse>('execute_tool', {
        toolName,
        params,
        timeout: timeout ?? null,
      });
      set({ lastCallResult: result, calling: false });
      // Refresh history after call
      get().fetchHistory();
      return result;
    } catch (e) {
      const errResult: ToolCallResponse = {
        success: false,
        error: String(e),
        duration_ms: 0,
      };
      set({ lastCallResult: errResult, calling: false });
      return errResult;
    }
  },

  fetchHistory: async () => {
    set({ historyLoading: true });
    try {
      const history = await invoke<ToolCallHistoryRecord[]>('get_tool_history');
      set({ history, historyLoading: false });
    } catch (e) {
      set({ error: String(e), historyLoading: false });
    }
  },
}));
