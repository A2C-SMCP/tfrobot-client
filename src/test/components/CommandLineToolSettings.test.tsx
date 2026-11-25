import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { CommandLineToolSettings } from '@/components/ComputerSettings/CommandLineToolSettings';
import { useComputerStore } from '@/stores/computerStore';
import { render } from '@/test/helpers/render';
import { PageHost } from '@/components/Navigation/PageHost';

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

  it('separates a status refresh from navigation to runtime recovery', async () => {
    vi.mocked(invoke).mockResolvedValue({
      ...disabledState, runtimeState: 'error', error: 'connection failed', policy: { enabled: true },
    });
    const onOpenRuntime = vi.fn();
    render(<CommandLineToolSettings computerId="computer-a" onOpenRuntime={onOpenRuntime} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole('button', { name: 'Runtime workbench' }));
    expect(onOpenRuntime).toHaveBeenCalledOnce();
    expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument();
  });

  it('persists the single feature switch and exposes pending start', async () => {
    render(<CommandLineToolSettings computerId="computer-a" />);
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable command line tool' }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      'update_command_line_tool_policy',
      { request: { computerId: 'computer-a', policy: { enabled: true } } },
    ));
    expect(await screen.findByText('Command line tool setting saved')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Change working directory' })).toBeEnabled();
  });

  it('persists a selected custom workspace without adding another switch', async () => {
    vi.mocked(open).mockReturnValue(Promise.resolve('/work/project'));
    render(<CommandLineToolSettings computerId="computer-a" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Change working directory' }));
    fireEvent.click(screen.getByLabelText('Custom directory'));
    fireEvent.click(screen.getByRole('button', { name: 'Choose workspace' }));
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Workspace' })).toHaveValue('/work/project'));
    expect(invoke).not.toHaveBeenCalledWith('update_command_line_tool_policy', expect.anything());
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' }));

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

  it.each([['success', false], ['success', true], ['failure', false], ['failure', true]] as const)('retains an automatic policy save result after delayed %s off screen (unmounted: %s)', async (outcome, released) => {
    let finish!: () => void;
    let stored = disabledState;
    const original = vi.mocked(invoke).getMockImplementation()!;
    let deferred = true;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === 'get_command_line_tool_state') return Promise.resolve(stored);
      if (command === 'update_command_line_tool_policy' && deferred) {
        deferred = false;
        return new Promise((resolve, reject) => {
          finish = () => {
            if (outcome === 'failure') reject(new Error('save unavailable'));
            else { stored = { ...disabledState, policy: { enabled: true }, runtimeState: 'pending' }; resolve(stored); }
          };
        });
      }
      return original(command, args);
    });
    const memory = new NavigationMemory();
    const tree = (active: boolean) => <NavigationMemoryProvider memory={memory}><NavigationScope id={released && !active ? 'computer:b' : 'computer:a'} key={released && !active ? 'b' : 'a'}>
      {released ? active ? <CommandLineToolSettings computerId="computer-a" /> : <div>Other Computer</div> : <PageHost name="command-line" active={active}><CommandLineToolSettings computerId="computer-a" /></PageHost>}
    </NavigationScope></NavigationMemoryProvider>;
    const view = render(tree(true));
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable command line tool' }));
    await waitFor(() => expect(finish).toBeDefined());
    view.rerender(tree(false));
    await act(async () => { finish(); });
    view.rerender(tree(true));
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent(outcome === 'success' ? /^Saved$/ : 'Save failed'));
    if (outcome === 'failure') {
      fireEvent.click(screen.getByRole('button', { name: 'Retry saving' }));
      await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent(/^Saved$/));
      expect(invoke).toHaveBeenLastCalledWith('update_command_line_tool_policy', expect.objectContaining({ request: { computerId: 'computer-a', policy: { enabled: true } } }));
    }
  }, 15_000);

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

      expect(await screen.findByRole('button', { name: 'Change working directory' })).toBeDisabled();
      expect(screen.queryByRole('textbox', { name: 'Workspace' })).not.toBeInTheDocument();
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
    fireEvent.click(await screen.findByRole('button', { name: 'Change working directory' }));
    fireEvent.click(screen.getByLabelText('Custom directory'));
    fireEvent.click(screen.getByRole('button', { name: 'Choose workspace' }));
    await act(async () => resolveSelection?.('/work/project'));
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(stateRequestCount).toBe(2));
    expect(within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' })).toBeDisabled();
    expect(invoke).not.toHaveBeenCalledWith(
      'update_command_line_tool_policy',
      expect.anything(),
    );
  });


it('explains an initial policy read failure without showing a fabricated off switch', async () => {
  let available = false;
  vi.mocked(invoke).mockImplementation(async () => {
    if (!available) throw new Error('policy read unavailable');
    return disabledState;
  });
  render(<CommandLineToolSettings computerId="computer-a" />);
  expect(await screen.findByText('Could not read the saved command-line policy.')).toBeInTheDocument();
  expect(screen.queryByRole('switch', { name: 'Enable command line tool' })).not.toBeInTheDocument();
  available = true;
  fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  expect(await screen.findByRole('switch', { name: 'Enable command line tool' })).toHaveAttribute('aria-checked', 'false');
});


  it('locks the complete workspace intent from the start of a delayed preflight', async () => {
    let finish!: (value: unknown) => void;
    let reads = 0;
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === 'get_command_line_tool_state') {
        reads += 1;
        return reads === 1 ? Promise.resolve(disabledState) : new Promise((resolve) => { finish = resolve; });
      }
      return original(command, args);
    });
    render(<CommandLineToolSettings computerId="computer-a" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Change working directory' }));
    fireEvent.click(screen.getByLabelText('Custom directory'));
    const editor = within(screen.getByRole('dialog'));
    const path = editor.getByRole('textbox', { name: 'Workspace' });
    fireEvent.change(path, { target: { value: '/old/path' } });
    fireEvent.click(editor.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(finish).toBeDefined());
    expect(path).toBeDisabled();
    expect(editor.getByLabelText('Custom directory')).toBeDisabled();
    expect(editor.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    expect(editor.getByRole('button', { name: /Save$/ })).toBeDisabled();
    expect(editor.getByRole('button', { name: 'Choose workspace' })).toBeDisabled();
    expect(invoke).not.toHaveBeenCalledWith('update_command_line_tool_policy', expect.anything());
    await act(async () => finish(disabledState));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('update_command_line_tool_policy', {
      request: { computerId: 'computer-a', policy: { enabled: false, workspace_root: '/old/path' } },
    }));
  });

});
