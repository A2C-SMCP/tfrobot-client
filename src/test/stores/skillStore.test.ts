import { invoke } from '@tauri-apps/api/core';
import { useSkillStore } from '@/stores/skillStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useSkillStore.getState().reset();
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

describe('skillStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  it('fetches active skills for an instance', async () => {
    const skills = [
      {
        name: 'example',
        source: 'user',
        path: '/tmp/skill_home/user/example',
        description: 'Example skill',
      },
    ];
    mockedInvoke.mockResolvedValueOnce(skills);

    await useSkillStore.getState().fetchSkills('computer-a');

    expect(mockedInvoke).toHaveBeenCalledWith('list_skills', { instanceId: 'computer-a' });
    expect(useSkillStore.getState().skills).toEqual(skills);
    expect(useSkillStore.getState().activeInstanceId).toBe('computer-a');
  });

  it('ignores stale skills from a previous instance', async () => {
    const first = deferred<Array<{ name: string; source: string; path: string; description: string }>>();
    const second = deferred<Array<{ name: string; source: string; path: string; description: string }>>();
    mockedInvoke.mockReturnValueOnce(first.promise as any);
    mockedInvoke.mockReturnValueOnce(second.promise as any);

    const firstFetch = useSkillStore.getState().fetchSkills('computer-a');
    const secondFetch = useSkillStore.getState().fetchSkills('computer-b');

    second.resolve([{ name: 'b', source: 'user', path: '/b', description: 'B' }]);
    await secondFetch;
    first.resolve([{ name: 'a', source: 'user', path: '/a', description: 'A' }]);
    await firstFetch;

    expect(useSkillStore.getState().skills[0].name).toBe('b');
    expect(useSkillStore.getState().activeInstanceId).toBe('computer-b');
  });

  it('selects SKILL.md through the backend resource API', async () => {
    mockedInvoke.mockResolvedValueOnce({
      name: 'example',
      relPath: 'SKILL.md',
      mimeType: 'text/markdown',
      totalSize: 10,
      sha256: 'abc',
      isEntry: true,
      isText: true,
      body: '# Example',
    });

    await useSkillStore.getState().selectSkill('computer-a', 'example');

    expect(mockedInvoke).toHaveBeenCalledWith('get_skill', {
      instanceId: 'computer-a',
      name: 'example',
      relPath: null,
    });
    expect(useSkillStore.getState().selectedSkill?.body).toBe('# Example');
  });

  it('refreshes skills without client filesystem scanning', async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);
    mockedInvoke.mockResolvedValueOnce([]);

    await useSkillStore.getState().refreshSkills('computer-a');

    expect(mockedInvoke).toHaveBeenCalledWith('refresh_skills', { instanceId: 'computer-a' });
    expect(mockedInvoke).toHaveBeenCalledWith('list_skills', { instanceId: 'computer-a' });
  });

  it('opens the saved Skill Home through the settings-specific backend command', async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);

    await useSkillStore.getState().openConfiguredLocalSkillsRoot('computer-a');

    expect(mockedInvoke).toHaveBeenCalledWith('open_configured_local_skills_root', {
      instanceId: 'computer-a',
    });
  });

  it('ignores stale refresh responses from a previous instance', async () => {
    const refreshA = deferred<void>();
    const fetchB = deferred<Array<{ name: string; source: string; path: string; description: string }>>();
    mockedInvoke.mockImplementation((command, args) => {
      if (command === 'refresh_skills' && (args as { instanceId: string }).instanceId === 'computer-a') {
        return refreshA.promise as any;
      }
      if (command === 'list_skills' && (args as { instanceId: string }).instanceId === 'computer-b') {
        return fetchB.promise as any;
      }
      return Promise.resolve([]);
    });

    const staleRefresh = useSkillStore.getState().refreshSkills('computer-a');
    const currentFetch = useSkillStore.getState().fetchSkills('computer-b');

    fetchB.resolve([{ name: 'b', source: 'user', path: '/b', description: 'B' }]);
    await currentFetch;
    refreshA.resolve();
    await staleRefresh;

    expect(useSkillStore.getState().activeInstanceId).toBe('computer-b');
    expect(useSkillStore.getState().skills[0].name).toBe('b');
  });

  it('loads marketplace capability and sends lifecycle requests to facade commands', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: {
        computerLifecycleApiAvailable: false,
        supportedOperations: [],
        requiredSdkApis: ['Computer::add_marketplace'],
        reason: 'unsupported',
      },
      marketplaces: [],
      plugins: [],
    });

    await useSkillStore.getState().fetchMarketplaceCapabilities('computer-a');

    expect(mockedInvoke).toHaveBeenCalledWith('get_marketplace_governance', {
      instanceId: 'computer-a',
    });
    expect(useSkillStore.getState().capabilities?.computerLifecycleApiAvailable).toBe(false);

    mockedInvoke.mockResolvedValueOnce(undefined);
    mockedInvoke.mockResolvedValueOnce(useSkillStore.getState().governance);
    mockedInvoke.mockResolvedValueOnce([]);
    await useSkillStore.getState().addMarketplace('computer-a', {
      name: 'tf',
      gitUrl: 'https://example.com/tf.git',
    });

    expect(mockedInvoke).toHaveBeenCalledWith('add_marketplace', {
      instanceId: 'computer-a',
      request: { name: 'tf', gitUrl: 'https://example.com/tf.git' },
    });
    expect(mockedInvoke).toHaveBeenCalledWith('list_skills', {
      instanceId: 'computer-a',
    });
  });

  it('describes a pending marketplace clone until its lifecycle refresh completes', async () => {
    const addMarketplace = deferred<void>();
    mockedInvoke.mockImplementation((command) => {
      if (command === 'add_marketplace') return addMarketplace.promise as any;
      if (command === 'get_marketplace_governance') {
        return Promise.resolve({
          capabilities: {
            computerLifecycleApiAvailable: true,
            supportedOperations: ['add_marketplace'],
            requiredSdkApis: [],
            reason: 'supported',
          },
          marketplaces: [],
          plugins: [],
        });
      }
      if (command === 'list_skills') return Promise.resolve([]);
      return Promise.resolve();
    });

    const pendingAdd = useSkillStore.getState().addMarketplace('computer-a', {
      name: 'tf',
      gitUrl: 'https://example.com/tf.git',
    });

    expect(useSkillStore.getState().marketplaceOperation).toMatchObject({
      kind: 'add',
      target: 'tf',
    });
    expect(useSkillStore.getState().loadingMarketplace).toBe(true);

    await useSkillStore.getState().fetchMarketplaceGovernance('computer-a');
    expect(useSkillStore.getState().loadingMarketplace).toBe(false);
    expect(useSkillStore.getState().marketplaceOperation).toMatchObject({
      kind: 'add',
      target: 'tf',
    });
    await expect(useSkillStore.getState().refreshMarketplace('computer-a', 'tf'))
      .rejects.toThrow('Marketplace operation already in progress: add tf');
    expect(mockedInvoke).not.toHaveBeenCalledWith('refresh_marketplace', {
      instanceId: 'computer-a',
      marketplace: 'tf',
    });

    addMarketplace.resolve();
    await pendingAdd;

    expect(useSkillStore.getState().marketplaceOperation).toBeNull();
    expect(useSkillStore.getState().loadingMarketplace).toBe(false);
  });

  it('stores readable marketplace lifecycle errors from structured backend failures', async () => {
    mockedInvoke.mockRejectedValueOnce({
      message: "MCP server 'audit-mcp' already exists as a user-managed MCP server",
    });

    await expect(useSkillStore.getState().installPlugin('computer-a', {
      marketplace: 'acme',
      plugin: 'audit',
    })).rejects.toEqual({
      message: "MCP server 'audit-mcp' already exists as a user-managed MCP server",
    });

    expect(mockedInvoke).toHaveBeenCalledWith('install_plugin', {
      instanceId: 'computer-a',
      request: {
        marketplace: 'acme',
        plugin: 'audit',
      },
    });
    expect(useSkillStore.getState().recordsByInstanceId['computer-a'].marketplaceError)
      .toBe("MCP server 'audit-mcp' already exists as a user-managed MCP server");
  });

  it('ignores stale marketplace lifecycle responses from a previous instance', async () => {
    const addA = deferred<void>();
    const fetchB = deferred<{
      capabilities: {
        computerLifecycleApiAvailable: boolean;
        supportedOperations: string[];
        requiredSdkApis: string[];
        reason: string;
      };
      marketplaces: [];
      plugins: [];
    }>();
    mockedInvoke.mockImplementation((command, args) => {
      if (command === 'add_marketplace' && (args as { instanceId: string }).instanceId === 'computer-a') {
        return addA.promise as any;
      }
      if (command === 'get_marketplace_governance' && (args as { instanceId: string }).instanceId === 'computer-b') {
        return fetchB.promise as any;
      }
      return Promise.resolve({
        capabilities: {
          computerLifecycleApiAvailable: false,
          supportedOperations: [],
          requiredSdkApis: [],
          reason: 'stale',
        },
        marketplaces: [],
        plugins: [],
      });
    });

    const staleAdd = useSkillStore.getState().addMarketplace('computer-a', {
      name: 'tf',
      gitUrl: 'https://example.com/tf.git',
    });
    const currentFetch = useSkillStore.getState().fetchMarketplaceCapabilities('computer-b');

    fetchB.resolve({
      capabilities: {
        computerLifecycleApiAvailable: true,
        supportedOperations: ['add_marketplace'],
        requiredSdkApis: [],
        reason: 'supported',
      },
      marketplaces: [],
      plugins: [],
    });
    await currentFetch;
    addA.resolve();
    await staleAdd;

    expect(useSkillStore.getState().activeInstanceId).toBe('computer-b');
    expect(useSkillStore.getState().capabilities?.reason).toBe('supported');
    expect(mockedInvoke).not.toHaveBeenCalledWith('list_skills', {
      instanceId: 'computer-a',
    });
    expect(useSkillStore.getState().recordsByInstanceId['computer-a'].marketplaceOperation)
      .toBeNull();
    expect(useSkillStore.getState().recordsByInstanceId['computer-a'].loadingMarketplace)
      .toBe(false);
  });

  it('propagates an inactive marketplace lifecycle failure to its original caller', async () => {
    const firstEnable = deferred<void>();
    let enableCalls = 0;
    mockedInvoke.mockImplementation((command) => {
      if (command === 'enable_plugin') {
        enableCalls += 1;
        return enableCalls === 1 ? firstEnable.promise as any : Promise.resolve();
      }
      if (command === 'get_marketplace_governance') {
        return Promise.resolve({
          capabilities: {
            computerLifecycleApiAvailable: true,
            supportedOperations: ['enable_plugin'],
            requiredSdkApis: [],
            reason: 'supported',
          },
          marketplaces: [],
          plugins: [],
        });
      }
      if (command === 'list_skills') return Promise.resolve([]);
      return Promise.resolve();
    });
    const request = { marketplace: 'acme', plugin: 'audit' };

    const staleEnable = useSkillStore.getState().enablePlugin('computer-a', request);
    const staleFailure = expect(staleEnable).rejects.toEqual({
      code: 'missing_secret',
      input_id: 'audit@acme/api_token',
    });
    await useSkillStore.getState().enablePlugin('computer-b', request);
    firstEnable.reject({
      code: 'missing_secret',
      input_id: 'audit@acme/api_token',
    });

    await staleFailure;
    expect(useSkillStore.getState().recordsByInstanceId['computer-a'].marketplaceError)
      .toBe('{"code":"missing_secret","input_id":"audit@acme/api_token"}');
  });

  it('keeps instance records isolated when switching back to a previous instance', async () => {
    mockedInvoke
      .mockResolvedValueOnce([{ name: 'a', source: 'user', path: '/a', description: 'A' }])
      .mockResolvedValueOnce([{ name: 'b', source: 'user', path: '/b', description: 'B' }]);

    await useSkillStore.getState().fetchSkills('computer-a');
    await useSkillStore.getState().fetchSkills('computer-b');

    useSkillStore.setState((state) => ({
      ...state,
      ...state.recordsByInstanceId['computer-a'],
      activeInstanceId: 'computer-a',
    }));

    expect(useSkillStore.getState().skills[0].name).toBe('a');
    expect(useSkillStore.getState().recordsByInstanceId['computer-b'].skills[0].name).toBe('b');
  });
});
