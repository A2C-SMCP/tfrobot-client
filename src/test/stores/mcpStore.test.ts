import { invoke } from '@tauri-apps/api/core';
import { useMcpStore, getConfigName, getConfigType, type McpServerConfig } from '@/stores/mcpStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useMcpStore.setState({ servers: [], loading: false, error: null });
}

describe('mcpStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchServers', () => {
    it('populates servers list', async () => {
      const mockServers = [{ name: 'srv', running: false, status_message: '', disabled: false }];
      mockedInvoke.mockResolvedValueOnce(mockServers);

      await useMcpStore.getState().fetchServers();

      expect(mockedInvoke).toHaveBeenCalledWith('get_mcp_servers');
      expect(useMcpStore.getState().servers).toEqual(mockServers);
      expect(useMcpStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('connection failed');

      await useMcpStore.getState().fetchServers();

      expect(useMcpStore.getState().error).toBe('connection failed');
    });
  });

  describe('addServer', () => {
    it('invokes add_mcp_server and refreshes', async () => {
      const config: McpServerConfig = {
        Stdio: { name: 'test', command: 'node', args: [], env: {}, disabled: false, forbidden_tools: [], tool_meta: {} },
      };
      mockedInvoke.mockResolvedValueOnce(undefined); // add_mcp_server
      mockedInvoke.mockResolvedValueOnce([]);         // fetchServers

      await useMcpStore.getState().addServer(config);

      expect(mockedInvoke).toHaveBeenCalledWith('add_mcp_server', { config });
    });

    it('sets error and re-throws on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('duplicate');

      await expect(
        useMcpStore.getState().addServer({ Stdio: { name: 'x', command: 'x', args: [], env: {}, disabled: false, forbidden_tools: [], tool_meta: {} } })
      ).rejects.toBe('duplicate');

      expect(useMcpStore.getState().error).toBe('duplicate');
    });
  });

  describe('removeServer', () => {
    it('invokes remove_mcp_server and refreshes', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().removeServer('test');

      expect(mockedInvoke).toHaveBeenCalledWith('remove_mcp_server', { name: 'test' });
    });
  });

  describe('startServer / stopServer', () => {
    it('start invokes start_mcp_server', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().startServer('srv');

      expect(mockedInvoke).toHaveBeenCalledWith('start_mcp_server', { name: 'srv' });
    });

    it('stop invokes stop_mcp_server', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().stopServer('srv');

      expect(mockedInvoke).toHaveBeenCalledWith('stop_mcp_server', { name: 'srv' });
    });
  });

  describe('startAll / stopAll', () => {
    it('startAll invokes start_all_servers', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().startAll();

      expect(mockedInvoke).toHaveBeenCalledWith('start_all_servers');
    });

    it('stopAll invokes stop_all_servers', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);

      await useMcpStore.getState().stopAll();

      expect(mockedInvoke).toHaveBeenCalledWith('stop_all_servers');
    });
  });

  describe('importConfig / exportConfig', () => {
    it('importConfig returns result and refreshes', async () => {
      const result = { servers_imported: 2, inputs_imported: 1, servers_skipped: [] };
      mockedInvoke.mockResolvedValueOnce(result); // import_config
      mockedInvoke.mockResolvedValueOnce([]);     // fetchServers

      const ret = await useMcpStore.getState().importConfig('/path/to/config.json');

      expect(mockedInvoke).toHaveBeenCalledWith('import_config', { path: '/path/to/config.json', format: null });
      expect(ret).toEqual(result);
    });

    it('exportConfig invokes export_config', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useMcpStore.getState().exportConfig('/out.json', ['srv1']);

      expect(mockedInvoke).toHaveBeenCalledWith('export_config', { path: '/out.json', serverNames: ['srv1'] });
    });

    it('exportConfig passes null when no server names', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useMcpStore.getState().exportConfig('/out.json');

      expect(mockedInvoke).toHaveBeenCalledWith('export_config', { path: '/out.json', serverNames: null });
    });
  });
});

describe('mcpStore helpers', () => {
  it('getConfigName extracts name from each variant', () => {
    expect(getConfigName({ Stdio: { name: 'a', command: '', args: [], env: {}, disabled: false, forbidden_tools: [], tool_meta: {} } })).toBe('a');
    expect(getConfigName({ Http: { name: 'b', url: '', headers: {}, disabled: false, forbidden_tools: [], tool_meta: {} } })).toBe('b');
    expect(getConfigName({ Sse: { name: 'c', url: '', headers: {}, disabled: false, forbidden_tools: [], tool_meta: {} } })).toBe('c');
  });

  it('getConfigType returns correct type string', () => {
    expect(getConfigType({ Stdio: { name: '', command: '', args: [], env: {}, disabled: false, forbidden_tools: [], tool_meta: {} } })).toBe('stdio');
    expect(getConfigType({ Http: { name: '', url: '', headers: {}, disabled: false, forbidden_tools: [], tool_meta: {} } })).toBe('http');
    expect(getConfigType({ Sse: { name: '', url: '', headers: {}, disabled: false, forbidden_tools: [], tool_meta: {} } })).toBe('sse');
  });
});
