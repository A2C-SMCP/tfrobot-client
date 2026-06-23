import { invoke } from '@tauri-apps/api/core';
import { useDebugStore, type ToolInfo } from '@/stores/debugStore';

const mockedInvoke = vi.mocked(invoke);
const instanceId = 'computer-a';

function resetStore() {
  useDebugStore.setState({
    tools: [],
    selectedTool: null,
    lastCallResult: null,
    calling: false,
    resources: [],
    resourcesLoading: false,
    resourcesNextCursor: null,
    toolsLoading: false,
    history: [],
    historyLoading: false,
    error: null,
    activeInstanceId: null,
    toolsRequestId: 0,
    resourcesRequestId: 0,
    historyRequestId: 0,
    executionRequestId: 0,
  });
}

const mockTool: ToolInfo = {
  name: 'read_file',
  description: 'Read a file',
  inputSchema: { type: 'object', properties: { path: { type: 'string' } } },
  server: 'fs-server',
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('debugStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchTools', () => {
    it('populates tools list', async () => {
      mockedInvoke.mockResolvedValueOnce([mockTool]);

      await useDebugStore.getState().fetchTools(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('get_available_tools', { instanceId });
      expect(useDebugStore.getState().tools).toEqual([mockTool]);
      expect(useDebugStore.getState().toolsLoading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('manager not init');

      await useDebugStore.getState().fetchTools(instanceId);

      expect(useDebugStore.getState().error).toBe('manager not init');
      expect(useDebugStore.getState().toolsLoading).toBe(false);
    });

    it('ignores stale tools responses from a previous computer instance', async () => {
      const first = deferred<ToolInfo[]>();
      const second = deferred<ToolInfo[]>();
      const otherTool = { ...mockTool, name: 'write_file' };
      mockedInvoke.mockReturnValueOnce(first.promise as any);
      mockedInvoke.mockReturnValueOnce(second.promise as any);

      const firstFetch = useDebugStore.getState().fetchTools('computer-a');
      const secondFetch = useDebugStore.getState().fetchTools('computer-b');

      second.resolve([otherTool]);
      await secondFetch;
      first.resolve([mockTool]);
      await firstFetch;

      expect(useDebugStore.getState().tools).toEqual([otherTool]);
      expect(useDebugStore.getState().activeInstanceId).toBe('computer-b');
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

  describe('fetchResources', () => {
    it('fetches resources for an instance and server', async () => {
      const response = {
        resources: [
          {
            server: 'fs-server',
            uri: 'file://readme',
            name: 'README.md',
            description: 'Project readme',
            mime_type: 'text/markdown',
          },
        ],
        next_cursor: 'next-page',
      };
      mockedInvoke.mockResolvedValueOnce(response);

      await useDebugStore.getState().fetchResources(instanceId, 'fs-server');

      expect(mockedInvoke).toHaveBeenCalledWith('get_debug_resources', {
        instanceId,
        serverName: 'fs-server',
        cursor: null,
      });
      expect(useDebugStore.getState().resources).toEqual(response.resources);
      expect(useDebugStore.getState().resourcesNextCursor).toBe('next-page');
      expect(useDebugStore.getState().resourcesLoading).toBe(false);
    });

    it('appends resources when loading the next page', async () => {
      useDebugStore.setState({
        activeInstanceId: instanceId,
        resources: [{ server: 'fs-server', uri: 'file://a', name: 'A' }],
        resourcesNextCursor: 'cursor-2',
      });
      mockedInvoke.mockResolvedValueOnce({
        resources: [{ server: 'fs-server', uri: 'file://b', name: 'B' }],
        next_cursor: null,
      });

      await useDebugStore.getState().fetchResources(instanceId, 'fs-server', 'cursor-2');

      expect(useDebugStore.getState().resources.map((resource) => resource.uri)).toEqual([
        'file://a',
        'file://b',
      ]);
      expect(useDebugStore.getState().resourcesNextCursor).toBeNull();
    });

    it('ignores stale resource responses from a previous computer instance', async () => {
      const first = deferred<any>();
      const second = deferred<any>();
      mockedInvoke.mockReturnValueOnce(first.promise);
      mockedInvoke.mockReturnValueOnce(second.promise);

      const firstFetch = useDebugStore.getState().fetchResources('computer-a', 'fs-server');
      const secondFetch = useDebugStore.getState().fetchResources('computer-b', 'fs-server');

      second.resolve({ resources: [{ server: 'fs-server', uri: 'file://b', name: 'B' }] });
      await secondFetch;
      first.resolve({ resources: [{ server: 'fs-server', uri: 'file://a', name: 'A' }] });
      await firstFetch;

      expect(useDebugStore.getState().resources).toEqual([
        { server: 'fs-server', uri: 'file://b', name: 'B' },
      ]);
      expect(useDebugStore.getState().activeInstanceId).toBe('computer-b');
      expect(useDebugStore.getState().resourcesLoading).toBe(false);
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

      const result = await useDebugStore.getState().executeTool(instanceId, 'read_file', { path: '/tmp' });

      expect(mockedInvoke).toHaveBeenCalledWith('execute_tool', {
        instanceId,
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

      await useDebugStore.getState().executeTool(instanceId, 'tool', {}, 30);

      expect(mockedInvoke).toHaveBeenCalledWith('execute_tool', {
        instanceId,
        toolName: 'tool',
        params: {},
        timeout: 30,
      });
    });

    it('returns error result on invoke failure', async () => {
      mockedInvoke.mockRejectedValueOnce('timeout exceeded');

      const result = await useDebugStore.getState().executeTool(instanceId, 'slow_tool', {});

      expect(result.success).toBe(false);
      expect(result.error).toBe('timeout exceeded');
      expect(result.duration_ms).toBe(0);
      expect(useDebugStore.getState().calling).toBe(false);
    });

    it('does not store a tool result after switching computer instances', async () => {
      const execute = deferred<any>();
      const otherTool = { ...mockTool, name: 'write_file' };
      mockedInvoke.mockReturnValueOnce(execute.promise);
      mockedInvoke.mockResolvedValueOnce([otherTool]);

      const executePromise = useDebugStore.getState().executeTool('computer-a', 'read_file', {});
      const switchPromise = useDebugStore.getState().fetchTools('computer-b');
      await switchPromise;

      const result = {
        success: true,
        result: { content: [{ type: 'text', text: 'hello' }], is_error: false },
        duration_ms: 42,
      };
      execute.resolve(result);
      await executePromise;

      expect(useDebugStore.getState().activeInstanceId).toBe('computer-b');
      expect(useDebugStore.getState().lastCallResult).toBeNull();
      expect(useDebugStore.getState().calling).toBe(false);
      expect(mockedInvoke).not.toHaveBeenCalledWith('get_tool_history', { instanceId: 'computer-a' });
    });
  });

  describe('fetchHistory', () => {
    it('populates history list', async () => {
      const mockHistory = [
        { timestamp: '2026-01-01T00:00:00Z', req_id: 'r1', server: 'fs', tool: 'read', parameters: {}, success: true },
      ];
      mockedInvoke.mockResolvedValueOnce(mockHistory);

      await useDebugStore.getState().fetchHistory(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('get_tool_history', { instanceId });
      expect(useDebugStore.getState().history).toEqual(mockHistory);
      expect(useDebugStore.getState().historyLoading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('not available');

      await useDebugStore.getState().fetchHistory(instanceId);

      expect(useDebugStore.getState().error).toBe('not available');
    });

    it('ignores stale history responses from a previous computer instance', async () => {
      const first = deferred<any[]>();
      const second = deferred<any[]>();
      const historyA = [{ timestamp: '2026-01-01T00:00:00Z', req_id: 'a', server: 'fs', tool: 'read', parameters: {}, success: true }];
      const historyB = [{ timestamp: '2026-01-01T00:00:00Z', req_id: 'b', server: 'fs', tool: 'write', parameters: {}, success: true }];
      mockedInvoke.mockReturnValueOnce(first.promise);
      mockedInvoke.mockReturnValueOnce(second.promise);

      const firstFetch = useDebugStore.getState().fetchHistory('computer-a');
      const secondFetch = useDebugStore.getState().fetchHistory('computer-b');

      second.resolve(historyB);
      await secondFetch;
      first.resolve(historyA);
      await firstFetch;

      expect(useDebugStore.getState().history).toEqual(historyB);
      expect(useDebugStore.getState().activeInstanceId).toBe('computer-b');
      expect(useDebugStore.getState().historyLoading).toBe(false);
    });
  });
});
