import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  ALL_PORTABLE_CONFIG_GROUPS,
  usePortableConfigStore,
} from '@/stores/portableConfigStore';

const mockedInvoke = vi.mocked(invoke);

describe('portableConfigStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    usePortableConfigStore.getState().reset();
  });

  it('exports a package with the selected groups', async () => {
    mockedInvoke.mockResolvedValueOnce(undefined);

    await usePortableConfigStore
      .getState()
      .exportPackage('computer-a', '/tmp/out.json', ALL_PORTABLE_CONFIG_GROUPS);

    expect(mockedInvoke).toHaveBeenCalledWith('export_computer_package', {
      instanceId: 'computer-a',
      path: '/tmp/out.json',
      groups: ALL_PORTABLE_CONFIG_GROUPS,
    });
  });

  it('inspects a package to prefill the create form', async () => {
    const inspection = {
      originalName: 'One',
      description: 'source',
      formatVersion: 1,
      groups: ['basic_profile'] as const,
    };
    mockedInvoke.mockResolvedValueOnce(inspection);

    const result = await usePortableConfigStore
      .getState()
      .inspectPackage('/tmp/package.json');

    expect(mockedInvoke).toHaveBeenCalledWith('inspect_computer_package', {
      path: '/tmp/package.json',
    });
    expect(result).toEqual(inspection);
  });
});
