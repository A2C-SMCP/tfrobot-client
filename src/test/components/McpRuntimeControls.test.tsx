import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { McpRuntimeControls } from '@/components/McpConfig/McpRuntimeControls';
import type { McpServerStatus } from '@/stores/mcpStore';

const userServer: McpServerStatus = {
  bundleId: 'runtime-server-id',
  name: 'runtime-server',
  activation_state: 'stopped',
  connection_state: 'disconnected',
  running: false,
  status_message: 'Stopped',
  disabled: false,
  managedBy: { type: 'user' },
};
const enabledCapability = { enabled: true, disabled_reason: null } as const;
const mockStore = {
  servers: [userServer] as McpServerStatus[],
  loading: false,
  error: null,
  fetchServers: vi.fn(),
  startServer: vi.fn(),
  stopServer: vi.fn(),
  startAll: vi.fn(),
  stopAll: vi.fn(),
};
const mockedInvoke = vi.mocked(invoke);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

vi.mock('@/stores/mcpStore', () => ({
  useMcpStore: vi.fn(() => mockStore),
}));

describe('McpRuntimeControls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedInvoke.mockReset();
    mockStore.servers = [userServer];
  });

  it('loads runtime status and exposes only lifecycle actions', () => {
    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);

    expect(mockStore.fetchServers).toHaveBeenCalledWith('computer-a');
    expect(screen.getByText('MCP Runtime')).toBeInTheDocument();
    expect(screen.getByText('runtime-server')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Start All$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Stop All$/ })).toBeInTheDocument();
    expect(screen.queryByText('Add Server')).not.toBeInTheDocument();
    expect(screen.queryByText('Import Config')).not.toBeInTheDocument();
    expect(screen.queryByText('Validate Schema')).not.toBeInTheDocument();
  }, 10000);

  it('disables lifecycle actions when the runtime cannot manage MCP servers', () => {
    const onStartRuntime = vi.fn();
    render(
      <McpRuntimeControls
        instanceId="computer-a"
        capability={{ enabled: false, disabled_reason: 'not_running' }}
        onStartRuntime={onStartRuntime}
      />,
    );

    expect(screen.getByRole('button', { name: /Start All$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Stop All$/ })).toBeDisabled();
    expect(screen.getByTitle('Start')).toBeDisabled();
    expect(screen.getByText('MCP lifecycle actions are unavailable')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Start Runtime' }));
    expect(onStartRuntime).toHaveBeenCalledOnce();
  });

  it('starts a server through the runtime action path', async () => {
    mockStore.startServer.mockResolvedValueOnce(undefined);
    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);

    fireEvent.click(screen.getByTitle('Start'));

    await waitFor(() => {
      expect(mockStore.startServer).toHaveBeenCalledWith('computer-a', 'runtime-server-id');
    });
    expect(await screen.findByText('Server runtime-server started')).toBeInTheDocument();
  });

  it('retries a failed connection by stopping before starting it again', async () => {
    const operations: string[] = [];
    mockStore.servers = [{
      ...userServer,
      activation_state: 'started',
      connection_state: 'error',
      running: true,
      status_message: 'error',
    }];
    mockStore.stopServer.mockImplementationOnce(async () => {
      operations.push('stop');
    });
    mockStore.startServer.mockImplementationOnce(async () => {
      operations.push('start');
    });
    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);

    fireEvent.click(screen.getByTitle('Retry connection'));

    expect(await screen.findByText('Server runtime-server reconnected')).toBeInTheDocument();
    expect(operations).toEqual(['stop', 'start']);
    expect(mockStore.stopServer).toHaveBeenCalledWith('computer-a', 'runtime-server-id');
    expect(mockStore.startServer).toHaveBeenCalledWith('computer-a', 'runtime-server-id');
    expect(screen.queryByText('Server runtime-server started')).not.toBeInTheDocument();
  });

  it('keeps a single-server technical failure out of the ordinary UI', async () => {
    mockStore.startServer.mockRejectedValueOnce({
      code: 'runtime_error',
      message: 'spawn /secret/path failed with token=private',
    });
    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);

    fireEvent.click(screen.getByTitle('Start'));

    expect(await screen.findByText(
      'The MCP operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/path|token=private/)).not.toBeInTheDocument();
  });

  it('advances through missing inputs and closes the prompt while MCP startup continues', async () => {
    const startup = deferred<void>();
    mockStore.startServer
      .mockRejectedValueOnce({
        code: 'missing_input',
        input_id: 'openrouterkey',
        env_hint: 'A2C_SMCP_openrouterkey',
        message: "Required value input 'openrouterkey' is unresolved",
      })
      .mockRejectedValueOnce({
        code: 'missing_input',
        input_id: 'zhipukey',
        env_hint: 'A2C_SMCP_zhipukey',
        message: "Required value input 'zhipukey' is unresolved",
      })
      .mockReturnValueOnce(startup.promise);
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === 'get_runtime_input') {
        const inputId = (args as { id: string }).id;
        return {
          type: 'PromptString',
          id: inputId,
          label: inputId,
          password: false,
        };
      }
      if (command === 'list_input_entries') return [];
      if (command === 'upsert_input_entry') return undefined;
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);
    fireEvent.click(screen.getByTitle('Start'));

    expect(await screen.findByText('Input required to start')).toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Key' })).toHaveValue('openrouterkey');
    fireEvent.change(await screen.findByPlaceholderText('Enter value'), {
      target: { value: 'test-openrouter-key' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(mockStore.startServer).toHaveBeenCalledTimes(2));
    await waitFor(() => {
      expect(screen.getByRole('textbox', { name: 'Key' })).toHaveValue('zhipukey');
    });

    fireEvent.change(await screen.findByPlaceholderText('Enter value'), {
      target: { value: 'test-zhipu-key' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(mockStore.startServer).toHaveBeenCalledTimes(3));
    expect(mockedInvoke).toHaveBeenCalledWith('upsert_input_entry', {
      instanceId: 'computer-a',
      key: 'openrouterkey',
      value: 'test-openrouter-key',
      secret: false,
    });
    expect(mockedInvoke).toHaveBeenCalledWith('upsert_input_entry', {
      instanceId: 'computer-a',
      key: 'zhipukey',
      value: 'test-zhipu-key',
      secret: false,
    });
    expect(screen.queryByRole('textbox', { name: 'Key' })).not.toBeInTheDocument();

    await act(async () => {
      startup.resolve();
      await startup.promise;
    });
    expect(await screen.findByText('Server runtime-server started')).toBeInTheDocument();
  });

  it('shows Plugin-owned diagnostics without lifecycle buttons and opens the matching Plugin', () => {
    const owner = {
      type: 'plugin' as const,
      marketplace: 'tf-market',
      plugin: 'desktop-tools',
      pluginId: 'plugin-1',
    };
    mockStore.servers = [{
      bundleId: 'plugin-server-id',
      name: 'plugin-server',
      activation_state: 'started',
      connection_state: 'connected',
      running: true,
      status_message: 'Connected with 3 tools',
      disabled: false,
      managedBy: owner,
    }];
    const onOpenPlugin = vi.fn();

    render(
      <McpRuntimeControls
        instanceId="computer-a"
        capability={enabledCapability}
        onOpenPlugin={onOpenPlugin}
      />,
    );

    expect(screen.getByText('Plugin: desktop-tools')).toBeInTheDocument();
    expect(screen.getByText('Marketplace: tf-market')).toBeInTheDocument();
    expect(screen.getByText('Available')).toBeInTheDocument();
    expect(screen.queryByText('Connected with 3 tools')).not.toBeInTheDocument();
    expect(screen.queryByTitle('Start')).not.toBeInTheDocument();
    expect(screen.queryByTitle('Stop')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Manage desktop-tools$/ }));
    expect(onOpenPlugin).toHaveBeenCalledWith(owner);
  });

  it('shows an unauthorized started Plugin server as waiting for authorization', () => {
    mockStore.servers = [{
      bundleId: 'plugin-oauth-server-id',
      name: 'plugin-oauth-server',
      activation_state: 'started',
      connection_state: 'authorization_required',
      running: true,
      status_message: 'authorization_required',
      disabled: false,
      managedBy: {
        type: 'plugin',
        marketplace: 'tf-market',
        plugin: 'desktop-tools',
        pluginId: 'desktop-tools@tf-market',
      },
      oauth_status: { state: 'unauthorized' },
      oauth_interaction: 'interactive',
    }];

    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);

    expect(screen.getByText('Waiting for authorization')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Authorize' })).toBeInTheDocument();
    expect(screen.queryByText('Stopped')).not.toBeInTheDocument();
  });

  it('reports mixed batch results with changed, unchanged, excluded, and failed counts', async () => {
    mockStore.startAll.mockResolvedValueOnce({
      candidate_count: 3,
      actual_operation_count: 1,
      unchanged_count: 1,
      excluded_plugin_owned_count: 2,
      failures: [{
        bundleId: 'broken',
        name: 'Broken server',
        error: { code: 'runtime_error', message: 'process exited' },
      }],
    });
    mockStore.fetchServers.mockResolvedValueOnce(undefined);
    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);

    fireEvent.click(screen.getByRole('button', { name: /Start All$/ }));

    expect(await screen.findByText(
      'Start complete: 3 candidates, 1 started, 1 unchanged, 2 plugin-managed excluded, 1 failed.',
    )).toBeInTheDocument();
    expect(screen.getByText(
      'Broken server (broken): operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText('process exited')).not.toBeInTheDocument();
  });

  it('reports partial stop failures with changed, unchanged, and excluded counts', async () => {
    mockStore.stopAll.mockResolvedValueOnce({
      candidate_count: 3,
      actual_operation_count: 1,
      unchanged_count: 1,
      excluded_plugin_owned_count: 2,
      failures: [{
        bundleId: 'broken',
        name: 'Broken server',
        error: { code: 'runtime_error', message: 'disconnect failed' },
      }],
    });
    mockStore.fetchServers.mockResolvedValueOnce(undefined);
    render(<McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />);

    fireEvent.click(screen.getByRole('button', { name: /Stop All$/ }));

    expect(await screen.findByText(
      'Stop complete: 3 candidates, 1 stopped, 1 unchanged, 2 plugin-managed excluded, 1 failed.',
    )).toBeInTheDocument();
    expect(screen.getByText(
      'Broken server (broken): operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText('disconnect failed')).not.toBeInTheDocument();
  });

  it('ignores start feedback and input prompts that finish after switching instances', async () => {
    const action = deferred<{
      candidate_count: number;
      actual_operation_count: number;
      unchanged_count: number;
      excluded_plugin_owned_count: number;
      failures: Array<{
        bundleId: string;
        name: string;
        error: {
          code: 'missing_input';
          input_id: string;
          env_hint: string;
          message: string;
        };
      }>;
    }>();
    mockStore.startAll.mockReturnValueOnce(action.promise);
    const view = render(
      <McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />,
    );
    fireEvent.click(screen.getByRole('button', { name: /Start All$/ }));

    view.rerender(
      <McpRuntimeControls instanceId="computer-b" capability={enabledCapability} />,
    );
    await act(async () => {
      action.resolve({
        candidate_count: 1,
        actual_operation_count: 0,
        unchanged_count: 0,
        excluded_plugin_owned_count: 0,
        failures: [{
          bundleId: 'needs-token',
          name: 'Needs token',
          error: {
            code: 'missing_input',
            input_id: 'runtime-token',
            env_hint: 'A2C_SMCP_runtime_token',
            message: 'Runtime token is required',
          },
        }],
      });
      await action.promise;
    });

    expect(screen.queryByText(/Start complete: 1 candidate/)).not.toBeInTheDocument();
    expect(screen.queryByText('Runtime token is required')).not.toBeInTheDocument();
  });

  it('ignores a single-start input prompt that finishes after switching instances', async () => {
    const action = deferred<void>();
    mockStore.startServer.mockReturnValueOnce(action.promise);
    const view = render(
      <McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />,
    );
    fireEvent.click(screen.getByTitle('Start'));

    view.rerender(
      <McpRuntimeControls instanceId="computer-b" capability={enabledCapability} />,
    );
    await act(async () => {
      action.reject({
        code: 'missing_input',
        input_id: 'runtime-token',
        env_hint: 'A2C_SMCP_runtime_token',
        message: 'Runtime token is required',
      });
      await action.promise.catch(() => undefined);
    });

    expect(screen.queryByText('Runtime token is required')).not.toBeInTheDocument();
  });

  it('ignores stop feedback that finishes after switching instances', async () => {
    const action = deferred<{
      candidate_count: number;
      actual_operation_count: number;
      unchanged_count: number;
      excluded_plugin_owned_count: number;
      failures: [];
    }>();
    mockStore.stopAll.mockReturnValueOnce(action.promise);
    const view = render(
      <McpRuntimeControls instanceId="computer-a" capability={enabledCapability} />,
    );
    fireEvent.click(screen.getByRole('button', { name: /Stop All$/ }));

    view.rerender(
      <McpRuntimeControls instanceId="computer-b" capability={enabledCapability} />,
    );
    await act(async () => {
      action.resolve({
        candidate_count: 1,
        actual_operation_count: 1,
        unchanged_count: 0,
        excluded_plugin_owned_count: 0,
        failures: [],
      });
      await action.promise;
    });

    expect(screen.queryByText(/Stop complete: 1 candidate/)).not.toBeInTheDocument();
  });
});
