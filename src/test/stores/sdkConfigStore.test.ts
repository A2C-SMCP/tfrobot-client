import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useSdkConfigStore } from '@/stores/sdkConfigStore';

const mockedInvoke = vi.mocked(invoke);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe('sdkConfigStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSdkConfigStore.getState().reset();
  });

  it('loads the SDK snapshot and schema validation together', async () => {
    const snapshot = {
      version: 1,
      revision: 'sha256:a',
      mcp: { servers: [] },
      provenance: {},
    };
    const validation = { valid: true, errors: [] };
    mockedInvoke.mockResolvedValueOnce({ snapshot, validation });

    await useSdkConfigStore.getState().fetchConfig('computer-a');

    expect(mockedInvoke).toHaveBeenNthCalledWith(
      1,
      'get_computer_config_state',
      { instanceId: 'computer-a' },
    );
    expect(useSdkConfigStore.getState()).toMatchObject({
      snapshot,
      validation,
      activeInstanceId: 'computer-a',
      loading: false,
      validating: false,
    });
  });

  it('ignores a stale response after switching Computer instances', async () => {
    const firstState = deferred<unknown>();
    mockedInvoke
      .mockReturnValueOnce(firstState.promise)
      .mockResolvedValueOnce({
        snapshot: {
          version: 1,
          revision: 'sha256:b',
          mcp: { servers: [] },
          provenance: {},
        },
        validation: { valid: true, errors: [] },
      });

    const first = useSdkConfigStore.getState().fetchConfig('computer-a');
    const second = useSdkConfigStore.getState().fetchConfig('computer-b');
    await second;
    firstState.resolve({
      snapshot: {
        version: 1,
        revision: 'sha256:a',
        mcp: { servers: [] },
        provenance: {},
      },
      validation: { valid: false, errors: [] },
    });
    await first;

    expect(useSdkConfigStore.getState().activeInstanceId).toBe('computer-b');
    expect(useSdkConfigStore.getState().snapshot?.revision).toBe('sha256:b');
    expect(useSdkConfigStore.getState().validation?.valid).toBe(true);
  });

  it('reruns schema validation with its matching snapshot revision', async () => {
    const snapshot = {
      version: 1,
      revision: 'sha256:a',
      mcp: { servers: [] },
      provenance: {},
    };
    useSdkConfigStore.setState({ snapshot, activeInstanceId: 'computer-a' });
    const refreshedSnapshot = { ...snapshot, revision: 'sha256:b' };
    mockedInvoke.mockResolvedValueOnce({
      snapshot: refreshedSnapshot,
      validation: {
        valid: false,
        errors: [{ scope: 'project', field: 'servers.bad', reason: 'invalid' }],
      },
    });

    await useSdkConfigStore.getState().validateConfig('computer-a');

    expect(useSdkConfigStore.getState().snapshot).toEqual(refreshedSnapshot);
    expect(useSdkConfigStore.getState().validation?.valid).toBe(false);
  });

  it('keeps snapshot and validation paired when an older fetch finishes last', async () => {
    const olderFetch = deferred<unknown>();
    const newerValidation = deferred<unknown>();
    mockedInvoke
      .mockReturnValueOnce(olderFetch.promise)
      .mockReturnValueOnce(newerValidation.promise);

    const fetch = useSdkConfigStore.getState().fetchConfig('computer-a');
    const validate = useSdkConfigStore.getState().validateConfig('computer-a');
    newerValidation.resolve({
      snapshot: {
        version: 1,
        revision: 'sha256:newer',
        mcp: { servers: [] },
        provenance: {},
      },
      validation: { valid: false, errors: [] },
    });
    await validate;
    olderFetch.resolve({
      snapshot: {
        version: 1,
        revision: 'sha256:older',
        mcp: { servers: [] },
        provenance: {},
      },
      validation: { valid: true, errors: [] },
    });
    await fetch;

    expect(useSdkConfigStore.getState()).toMatchObject({
      snapshot: { revision: 'sha256:newer' },
      validation: { valid: false },
      loading: false,
      validating: false,
    });
  });

  it('uses config-only CRUD commands and refreshes the SDK snapshot', async () => {
    const config = {
      type: 'Stdio' as const,
      name: 'config-only',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { command: '${input:missing-command}', args: [], env: {} },
    };
    mockedInvoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        snapshot: {
          version: 1,
          revision: 'sha256:updated',
          mcp: { servers: [] },
          provenance: {},
        },
        validation: { valid: true, errors: [] },
      });

    await useSdkConfigStore.getState().upsertServer('computer-a', config);

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'upsert_computer_mcp_config', {
      instanceId: 'computer-a',
      config,
    });
    expect(mockedInvoke.mock.calls.flatMap(([command]) => command)).not.toContain('add_mcp_server');
    expect(useSdkConfigStore.getState().snapshot?.revision).toBe('sha256:updated');
  });

  it('imports through the config boundary and never fetches runtime status', async () => {
    const result = { servers_imported: 2, inputs_imported: 1, servers_skipped: [] };
    mockedInvoke
      .mockResolvedValueOnce(result)
      .mockResolvedValueOnce({
        snapshot: {
          version: 1,
          revision: 'sha256:imported',
          mcp: { servers: [] },
          provenance: {},
        },
        validation: { valid: true, errors: [] },
      });

    await expect(
      useSdkConfigStore.getState().importConfig('computer-a', '/path/to/config.json'),
    ).resolves.toEqual(result);

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'import_config', {
      path: '/path/to/config.json',
      instanceId: 'computer-a',
      format: null,
    });
    expect(mockedInvoke.mock.calls.flatMap(([command]) => command)).not.toContain('get_mcp_servers');
  });

  it('exports selected SDK configuration declarations', async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);

    await useSdkConfigStore.getState().exportConfig('computer-a', '/out.json', ['srv1']);

    expect(mockedInvoke).toHaveBeenCalledWith('export_config', {
      path: '/out.json',
      instanceId: 'computer-a',
      serverNames: ['srv1'],
    });
  });

  it('does not let a completed mutation switch the active Computer back', async () => {
    const mutation = deferred<void>();
    mockedInvoke
      .mockReturnValueOnce(mutation.promise)
      .mockResolvedValueOnce({
        snapshot: {
          version: 1,
          revision: 'sha256:b',
          mcp: { servers: [] },
          provenance: {},
        },
        validation: { valid: true, errors: [] },
      });
    useSdkConfigStore.setState({ activeInstanceId: 'computer-a' });

    const pending = useSdkConfigStore.getState().upsertServer('computer-a', {
      type: 'Stdio',
      name: 'late-a',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { command: 'node', args: [], env: {} },
    });
    await useSdkConfigStore.getState().fetchConfig('computer-b');
    mutation.resolve(undefined);
    await pending;

    expect(useSdkConfigStore.getState()).toMatchObject({
      activeInstanceId: 'computer-b',
      snapshot: { revision: 'sha256:b' },
      error: null,
    });
    expect(mockedInvoke).toHaveBeenCalledTimes(2);
  });

  it('refreshes after every same-instance mutation even when they complete out of order', async () => {
    const firstMutation = deferred<void>();
    const secondMutation = deferred<void>();
    mockedInvoke
      .mockReturnValueOnce(firstMutation.promise)
      .mockReturnValueOnce(secondMutation.promise)
      .mockResolvedValueOnce({
        snapshot: {
          version: 1,
          revision: 'sha256:second-only',
          mcp: { servers: [] },
          provenance: {},
        },
        validation: { valid: true, errors: [] },
      })
      .mockResolvedValueOnce({
        snapshot: {
          version: 1,
          revision: 'sha256:both',
          mcp: { servers: [] },
          provenance: {},
        },
        validation: { valid: true, errors: [] },
      });
    useSdkConfigStore.setState({ activeInstanceId: 'computer-a' });
    const config = {
      type: 'Stdio' as const,
      name: 'concurrent',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { command: 'node', args: [], env: {} },
    };

    const first = useSdkConfigStore.getState().upsertServer('computer-a', config);
    const second = useSdkConfigStore.getState().upsertServer('computer-a', config);
    secondMutation.resolve(undefined);
    await second;
    expect(useSdkConfigStore.getState().loading).toBe(true);
    firstMutation.resolve(undefined);
    await first;

    expect(useSdkConfigStore.getState()).toMatchObject({
      activeInstanceId: 'computer-a',
      snapshot: { revision: 'sha256:both' },
      loading: false,
      pendingMutations: {},
    });
  });

  it('does not surface a stale mutation error on another Computer', async () => {
    const mutation = deferred<void>();
    mockedInvoke
      .mockReturnValueOnce(mutation.promise)
      .mockResolvedValueOnce({
        snapshot: {
          version: 1,
          revision: 'sha256:b',
          mcp: { servers: [] },
          provenance: {},
        },
        validation: { valid: true, errors: [] },
      });
    useSdkConfigStore.setState({ activeInstanceId: 'computer-a' });
    const pending = useSdkConfigStore.getState().removeServer('computer-a', 'late-a');
    await useSdkConfigStore.getState().fetchConfig('computer-b');
    mutation.reject('A failed');

    await expect(pending).rejects.toBe('A failed');
    expect(useSdkConfigStore.getState()).toMatchObject({
      activeInstanceId: 'computer-b',
      snapshot: { revision: 'sha256:b' },
      error: null,
    });
  });
});
