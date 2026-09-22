import { fireEvent, render, screen, waitFor } from '../helpers/render';
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
        onCloseExport={() => {}}
      />,
    );

    for (const label of [
      /basic profile/i,
      /mcp configuration and input definitions/i,
      /non-sensitive input values/i,
      /skills and plugins/i,
    ]) {
      expect(screen.getByRole('checkbox', { name: label })).toBeChecked();
    }
  });

  it('exports the selected groups through the store', async () => {
    const { save } = await import('@tauri-apps/plugin-dialog');
    vi.mocked(save).mockResolvedValueOnce('/tmp/out.json');
    mockedInvoke.mockResolvedValueOnce(undefined);

    render(
      <ComputerPortableConfig
        instance={instance()}
        exportOpen
        onCloseExport={() => {}}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Export' }));

    await waitFor(() =>
      expect(mockedInvoke).toHaveBeenCalledWith('export_computer_package', {
        instanceId: 'computer-a',
        path: '/tmp/out.json',
        groups: [
          'basic_profile',
          'mcp_and_inputs',
          'non_sensitive_input_values',
          'skills_and_plugins',
        ],
      }),
    );
  });
});
