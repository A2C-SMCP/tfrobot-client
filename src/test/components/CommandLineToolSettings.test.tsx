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
    expect(screen.getByRole('textbox', { name: 'Workspace' })).toBeEnabled();
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

  it.each(['starting', 'running'] as const)(
    'locks workspace changes while the command line tool is %s',
    async (runtimeState) => {
      vi.mocked(invoke).mockImplementation(async (command) => {
        if (command === 'get_command_line_tool_state') {
          return {
            ...disabledState,
            policy: { enabled: true, workspace_root: '/work/project' },
            effectiveWorkspace: '/work/project',
            runtimeState,
          };
        }
        throw new Error(`unexpected command: ${command}`);
      });

      render(<CommandLineToolSettings computerId="computer-a" />);

      expect(await screen.findByRole('textbox', { name: 'Workspace' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Choose workspace' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Use default' })).toBeDisabled();
      expect(screen.getByText(
        'The workspace cannot be changed while the command line tool is running. '
        + 'Turn it off to make changes.',
      )).toBeInTheDocument();
      expect(open).not.toHaveBeenCalled();
    },
  );

  it('rechecks runtime state before applying a workspace selected from an open dialog', async () => {
    let resolveSelection: ((path: string) => void) | undefined;
    vi.mocked(open).mockReturnValue(new Promise((resolve) => {
      resolveSelection = resolve;
    }));
    let stateRequestCount = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'get_command_line_tool_state') {
        stateRequestCount += 1;
        return stateRequestCount === 1
          ? disabledState
          : { ...disabledState, policy: { enabled: true }, runtimeState: 'running' };
      }
      throw new Error(`unexpected command: ${command}`);
    });

    render(<CommandLineToolSettings computerId="computer-a" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Choose workspace' }));
    resolveSelection?.('/work/project');

    await waitFor(() => expect(stateRequestCount).toBe(2));
    expect(screen.getByRole('textbox', { name: 'Workspace' })).toBeDisabled();
    expect(invoke).not.toHaveBeenCalledWith(
      'update_command_line_tool_policy',
      expect.anything(),
    );
  });
});
