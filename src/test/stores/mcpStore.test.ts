import { invoke } from '@tauri-apps/api/core';
import { useMcpStore, getConfigName, getConfigType, type McpServerConfig } from '@/stores/mcpStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useMcpStore.setState({
    servers: [],
    loading: false,
    error: null,
    activeInstanceId: null,
    serversRequestId: 0,
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('mcpStore', () => {
  const instanceId = 'computer-a';

  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchServers', () => {
    it('populates servers list', async () => {
      const mockServers = [
        {
          name: 'srv',
          running: false,
          status_message: '',
          disabled: false,
          managedBy: { type: 'user' },
        },
      ];
      mockedInvoke.mockResolvedValueOnce(mockServers);

      await useMcpStore.getState().fetchServers(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('get_mcp_servers', { instanceId });
      expect(useMcpStore.getState().servers).toEqual(mockServers);
      expect(useMcpStore.getState().loading).toBe(false);
    });

    it('preserves plugin ownership metadata from backend status', async () => {
      const mockServers = [
        {
          name: 'plugin-srv',
          running: false,
          status_message: '',
          disabled: false,
          managedBy: {
            type: 'plugin',
            marketplace: 'tf-market',
            plugin: 'desktop-tools',
            pluginId: 'plugin-1',
          },
        },
      ];
      mockedInvoke.mockResolvedValueOnce(mockServers);

      await useMcpStore.getState().fetchServers(instanceId);

      expect(useMcpStore.getState().servers).toEqual(mockServers);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('connection failed');

      await useMcpStore.getState().fetchServers(instanceId);

      expect(useMcpStore.getState().error).toBe('connection failed');
    });

    it('ignores stale server responses from a previous computer instance', async () => {
      const first = deferred<
        Array<{
          name: string;
          running: boolean;
          status_message: string;
          disabled: boolean;
          managedBy: { type: 'user' };
        }>
      >();
      const second = deferred<
        Array<{
          name: string;
          running: boolean;
          status_message: string;
          disabled: boolean;
          managedBy: { type: 'user' };
        }>
      >();
      const serversA = [
        {
          name: 'a-only',
          running: true,
          status_message: 'Running',
          disabled: false,
          managedBy: { type: 'user' as const },
        },
      ];
      const serversB = [
        {
          name: 'b-only',
          running: true,
          status_message: 'Running',
          disabled: false,
          managedBy: { type: 'user' as const },
        },
      ];
      mockedInvoke.mockReturnValueOnce(first.promise as any);
      mockedInvoke.mockReturnValueOnce(second.promise as any);

      const firstFetch = useMcpStore.getState().fetchServers('computer-a');
      const secondFetch = useMcpStore.getState().fetchServers('computer-b');

      second.resolve(serversB);
      await secondFetch;
      first.resolve(serversA);
      await firstFetch;

      expect(useMcpStore.getState().servers).toEqual(serversB);
      expect(useMcpStore.getState().activeInstanceId).toBe('computer-b');
      expect(useMcpStore.getState().loading).toBe(false);
    });
  });

  describe('startServer / stopServer', () => {
    it('start invokes start_mcp_server', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().startServer(instanceId, 'srv');

      expect(mockedInvoke).toHaveBeenCalledWith('start_mcp_server', { instanceId, bundleId: 'srv' });
    });

    it('stop invokes stop_mcp_server', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().stopServer(instanceId, 'srv');

      expect(mockedInvoke).toHaveBeenCalledWith('stop_mcp_server', { instanceId, bundleId: 'srv' });
    });
  });

  describe('startAll / stopAll', () => {
    it('startAll invokes start_all_servers', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().startAll(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('start_all_servers', { instanceId });
    });

    it('stopAll invokes stop_all_servers', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().stopAll(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('stop_all_servers', { instanceId });
    });
  });

});

describe('mcpStore helpers', () => {
  it('getConfigName extracts name from each variant', () => {
    const stdio: McpServerConfig = {
      type: 'Stdio', name: 'a', disabled: false, forbidden_tools: [], tool_meta: {},
      server_parameters: { command: '', args: [], env: {} },
    };
    const http: McpServerConfig = {
      type: 'Http', name: 'b', disabled: false, forbidden_tools: [], tool_meta: {},
      server_parameters: { url: '', headers: {} },
    };
    const sse: McpServerConfig = {
      type: 'Sse', name: 'c', disabled: false, forbidden_tools: [], tool_meta: {},
      server_parameters: { url: '', headers: {} },
    };
    expect(getConfigName(stdio)).toBe('a');
    expect(getConfigName(http)).toBe('b');
    expect(getConfigName(sse)).toBe('c');
  });

  it('getConfigType returns correct type string', () => {
    const stdio: McpServerConfig = {
      type: 'Stdio', name: '', disabled: false, forbidden_tools: [], tool_meta: {},
      server_parameters: { command: '', args: [], env: {} },
    };
    const http: McpServerConfig = {
      type: 'Http', name: '', disabled: false, forbidden_tools: [], tool_meta: {},
      server_parameters: { url: '', headers: {} },
    };
    const sse: McpServerConfig = {
      type: 'Sse', name: '', disabled: false, forbidden_tools: [], tool_meta: {},
      server_parameters: { url: '', headers: {} },
    };
    expect(getConfigType(stdio)).toBe('stdio');
    expect(getConfigType(http)).toBe('http');
    expect(getConfigType(sse)).toBe('sse');
  });
});

describe('McpServerConfig JSON contract', () => {
  it('Stdio config has type field and server_parameters, no external tag key', () => {
    const config: McpServerConfig = {
      type: 'Stdio',
      name: 'test-stdio',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { command: 'npx', args: ['-y', 'server'], env: { KEY: 'val' }, cwd: '/tmp' },
    };
    const json = JSON.parse(JSON.stringify(config));

    // Must have internally tagged "type" field
    expect(json.type).toBe('Stdio');
    expect(json.name).toBe('test-stdio');
    // server_parameters must be nested
    expect(json.server_parameters).toBeDefined();
    expect(json.server_parameters.command).toBe('npx');
    expect(json.server_parameters.args).toEqual(['-y', 'server']);
    expect(json.server_parameters.env).toEqual({ KEY: 'val' });
    // Must NOT have externally tagged wrapper key
    expect(json.Stdio).toBeUndefined();
    expect(json.Http).toBeUndefined();
    expect(json.Sse).toBeUndefined();
  });

  it('Http config has type field and server_parameters', () => {
    const config: McpServerConfig = {
      type: 'Http',
      name: 'test-http',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { url: 'https://example.com', headers: { Authorization: 'Bearer tok' } },
    };
    const json = JSON.parse(JSON.stringify(config));

    expect(json.type).toBe('Http');
    expect(json.server_parameters.url).toBe('https://example.com');
    expect(json.server_parameters.headers).toEqual({ Authorization: 'Bearer tok' });
    expect(json.Http).toBeUndefined();
  });

  it('Sse config has type field and server_parameters', () => {
    const config: McpServerConfig = {
      type: 'Sse',
      name: 'test-sse',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { url: 'https://sse.example.com', headers: {} },
    };
    const json = JSON.parse(JSON.stringify(config));

    expect(json.type).toBe('Sse');
    expect(json.server_parameters.url).toBe('https://sse.example.com');
    expect(json.Sse).toBeUndefined();
  });
});
