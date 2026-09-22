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

  it('previews an import package', async () => {
    const preview = {
      originalName: 'One',
      finalName: 'One (2)',
      nameConflict: true,
      formatVersion: 1,
      versionCompatible: true,
      sections: [],
      marketplaces: [],
      installedPlugins: [],
    };
    mockedInvoke.mockResolvedValueOnce(preview);

    const result = await usePortableConfigStore
      .getState()
      .previewImport('/tmp/package.json');

    expect(mockedInvoke).toHaveBeenCalledWith('preview_computer_package_import', {
      path: '/tmp/package.json',
    });
    expect(result).toEqual(preview);
  });

  it('commits an import with the final name', async () => {
    mockedInvoke.mockResolvedValueOnce({ id: 'computer-new', name: 'Two' });

    const result = await usePortableConfigStore
      .getState()
      .commitImport('/tmp/package.json', 'Two');

    expect(mockedInvoke).toHaveBeenCalledWith('commit_computer_package_import', {
      path: '/tmp/package.json',
      finalName: 'Two',
    });
    expect(result).toEqual({ id: 'computer-new', name: 'Two' });
  });
});
