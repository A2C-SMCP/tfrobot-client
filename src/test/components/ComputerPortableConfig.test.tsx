import { fireEvent, render, screen } from '../helpers/render';
import { ComputerPortableConfig } from '@/components/Computer/ComputerPortableConfig';
import type { ComputerInstance } from '@/stores/computerStore';
import { usePortableConfigStore } from '@/stores/portableConfigStore';
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
  open: vi.fn(),
}));

function instance(): ComputerInstance {
  return {
    id: 'computer-a',
    name: 'Computer A',
    description: 'Primary',
    status: 'running',
    connectionStatus: 'disconnected',
    defaultSkillHome: '/app/computer-a/skill_home',
    configuredSkillHome: '/app/computer-a/skill_home',
    effectiveSkillHome: '/app/computer-a/skill_home',
    connectionPolicy: { target: null, auto_connect: false },
    remoteControl: {
      enabled: false,
      tool_scope: { mode: 'all' },
      target_scope: { mode: 'self_only' },
    },
    commandLine: { enabled: false },
    mcpStartConcurrency: 5,
    mcpServerCount: 0,
    runtime: runtimeSnapshot({ lifecycle: 'started' }),
  };
}

describe('ComputerPortableConfig', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    usePortableConfigStore.getState().reset();
  });

  it('shows all export groups selected by default', () => {
    render(
      <ComputerPortableConfig
        instance={instance()}
        exportOpen
        importOpen={false}
        onCloseExport={() => {}}
        onCloseImport={() => {}}
      />,
    );

    const groupLabels = [
      /basic profile/i,
      /mcp configuration and input definitions/i,
      /non-sensitive input values/i,
      /skills and plugins/i,
    ];
    for (const label of groupLabels) {
      const checkbox = screen.getByRole('checkbox', {
        name: label,
      });
      expect(checkbox).toBeChecked();
    }
  });

  it('previews an import package and lists marketplace sources', async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const mockedOpen = vi.mocked(open);
    mockedOpen.mockResolvedValueOnce('/tmp/package.json');
    mockedInvoke.mockResolvedValueOnce({
      originalName: 'Remote',
      finalName: 'Remote',
      nameConflict: false,
      formatVersion: 1,
      versionCompatible: true,
      sections: [],
      marketplaces: [{ name: 'acme', source: { type: 'git', url: 'https://git.example/acme' } }],
      installedPlugins: ['audit@acme'],
    });

    render(
      <ComputerPortableConfig
        instance={instance()}
        exportOpen={false}
        importOpen
        onCloseExport={() => {}}
        onCloseImport={() => {}}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Choose configuration file' }));

    expect(await screen.findByText('Remote')).toBeTruthy();
    expect(await screen.findByText(/https:\/\/git\.example\/acme/)).toBeTruthy();
    expect(await screen.findByText('audit@acme')).toBeTruthy();
    expect(
      await screen.findByText(/This package declares external Marketplace sources and plugins/),
    ).toBeTruthy();
  });
});
