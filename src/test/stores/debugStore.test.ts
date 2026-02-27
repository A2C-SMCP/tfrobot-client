import { invoke } from '@tauri-apps/api/core';
import { useDebugStore, type ToolInfo } from '@/stores/debugStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useDebugStore.setState({
    tools: [],
    selectedTool: null,
    lastCallResult: null,
    calling: false,
    toolsLoading: false,
    history: [],
    historyLoading: false,
    error: null,
  });
}

const mockTool: ToolInfo = {
  name: 'read_file',
  description: 'Read a file',
  inputSchema: { type: 'object', properties: { path: { type: 'string' } } },
  server: 'fs-server',
};

describe('debugStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchTools', () => {
    it('populates tools list', async () => {
      mockedInvoke.mockResolvedValueOnce([mockTool]);

      await useDebugStore.getState().fetchTools();

      expect(mockedInvoke).toHaveBeenCalledWith('get_available_tools');
      expect(useDebugStore.getState().tools).toEqual([mockTool]);
      expect(useDebugStore.getState().toolsLoading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('manager not init');

      await useDebugStore.getState().fetchTools();

      expect(useDebugStore.getState().error).toBe('manager not init');
      expect(useDebugStore.getState().toolsLoading).toBe(false);
    });
  });

  describe('selectTool', () => {
    it('updates selectedTool and clears lastCallResult', () => {
      useDebugStore.setState({ lastCallResult: { success: true, duration_ms: 10 } as any });

      useDebugStore.getState().selectTool(mockTool);

      expect(useDebugStore.getState().selectedTool).toEqual(mockTool);
      expect(useDebugStore.getState().lastCallResult).toBeNull();
    });

    it('accepts null to deselect', () => {
      useDebugStore.setState({ selectedTool: mockTool });

      useDebugStore.getState().selectTool(null);

      expect(useDebugStore.getState().selectedTool).toBeNull();
    });
  });

  describe('executeTool', () => {
    it('calls invoke and stores successful result', async () => {
      const mockResult = {
        success: true,
        result: { content: [{ type: 'text', text: 'hello' }], is_error: false },
        duration_ms: 42,
      };
      mockedInvoke.mockResolvedValueOnce(mockResult);  // execute_tool
      mockedInvoke.mockResolvedValueOnce([]);           // fetchHistory (auto-triggered)

      const result = await useDebugStore.getState().executeTool('read_file', { path: '/tmp' });

      expect(mockedInvoke).toHaveBeenCalledWith('execute_tool', {
        toolName: 'read_file',
        params: { path: '/tmp' },
        timeout: null,
      });
      expect(result).toEqual(mockResult);
      expect(useDebugStore.getState().lastCallResult).toEqual(mockResult);
      expect(useDebugStore.getState().calling).toBe(false);
    });

    it('passes timeout when provided', async () => {
      mockedInvoke.mockResolvedValueOnce({ success: true, duration_ms: 0 });
      mockedInvoke.mockResolvedValueOnce([]);

      await useDebugStore.getState().executeTool('tool', {}, 30);

      expect(mockedInvoke).toHaveBeenCalledWith('execute_tool', {
        toolName: 'tool',
        params: {},
        timeout: 30,
      });
    });

    it('returns error result on invoke failure', async () => {
      mockedInvoke.mockRejectedValueOnce('timeout exceeded');

      const result = await useDebugStore.getState().executeTool('slow_tool', {});

      expect(result.success).toBe(false);
      expect(result.error).toBe('timeout exceeded');
      expect(result.duration_ms).toBe(0);
      expect(useDebugStore.getState().calling).toBe(false);
    });
  });

  describe('fetchHistory', () => {
    it('populates history list', async () => {
      const mockHistory = [
        { timestamp: '2026-01-01T00:00:00Z', req_id: 'r1', server: 'fs', tool: 'read', parameters: {}, success: true },
      ];
      mockedInvoke.mockResolvedValueOnce(mockHistory);

      await useDebugStore.getState().fetchHistory();

      expect(mockedInvoke).toHaveBeenCalledWith('get_tool_history');
      expect(useDebugStore.getState().history).toEqual(mockHistory);
      expect(useDebugStore.getState().historyLoading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('not available');

      await useDebugStore.getState().fetchHistory();

      expect(useDebugStore.getState().error).toBe('not available');
    });
  });
});
