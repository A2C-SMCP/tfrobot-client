import { invoke } from '@tauri-apps/api/core';
import { useSkillStore } from '@/stores/skillStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useSkillStore.getState().reset();
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
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
    await useSkillStore.getState().addMarketplace('computer-a', {
      name: 'tf',
      gitUrl: 'https://example.com/tf.git',
    });

    expect(mockedInvoke).toHaveBeenCalledWith('add_marketplace', {
      instanceId: 'computer-a',
      request: { name: 'tf', gitUrl: 'https://example.com/tf.git' },
    });
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
