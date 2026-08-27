import { fireEvent, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { CommandLineToolSettings } from '@/components/ComputerSettings/CommandLineToolSettings';
import { useComputerStore } from '@/stores/computerStore';
import { render } from '@/test/helpers/render';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const disabledState = {
  policy: { enabled: false },
  effectiveWorkspace: '/client/computer-a/workspace',
  runtimeState: 'disabled',
  assetsAvailable: true,
};

describe('CommandLineToolSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useComputerStore.setState({ fetchInstances: vi.fn().mockResolvedValue(undefined) });
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'get_command_line_tool_state') return disabledState;
      if (command === 'update_command_line_tool_policy') {
        const request = (args as { request: { policy: { enabled: boolean; workspace_root?: string } } }).request;
        return {
          ...disabledState,
          policy: request.policy,
          runtimeState: request.policy.enabled ? 'pending' : 'disabled',
          effectiveWorkspace: request.policy.workspace_root ?? disabledState.effectiveWorkspace,
        };
      }
      throw new Error(`unexpected command: ${command}`);
    });
  });

  it('persists the single feature switch and exposes pending start', async () => {
    render(<CommandLineToolSettings computerId="computer-a" />);
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable command line tool' }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      'update_command_line_tool_policy',
      { request: { computerId: 'computer-a', policy: { enabled: true } } },
    ));
    expect(await screen.findByText('Pending start')).toBeInTheDocument();
  });

  it('persists a selected custom workspace without adding another switch', async () => {
    vi.mocked(open).mockReturnValue(Promise.resolve('/work/project'));
    render(<CommandLineToolSettings computerId="computer-a" />);
    const label = await screen.findByText('Choose workspace');
    fireEvent.click(label.closest('button')!);

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      'update_command_line_tool_policy',
      {
        request: {
          computerId: 'computer-a',
          policy: { enabled: false, workspace_root: '/work/project' },
        },
      },
    ));
    expect(screen.getAllByRole('switch')).toHaveLength(1);
  });
});
