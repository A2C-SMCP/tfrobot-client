import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';
import { PageHost } from '@/components/Navigation/PageHost';
import { act, fireEvent, render, screen, waitFor, within } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { SkillsSettings } from '@/components/ComputerSettings/SkillsSettings';
import { runtimeSnapshot } from '../helpers/store';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { useSkillStore } from '@/stores/skillStore';

vi.setConfig({ testTimeout: 15_000 });

const mockInvoke = vi.mocked(invoke);
const mockOpen = vi.mocked(open);
const instance: ComputerInstance = {
  id: 'computer-a', name: 'Computer A', status: 'running', connectionStatus: 'disconnected',
  localSkillsRoot: '/custom/skill-home', defaultSkillHome: '/app/computer_instances/computer-a/skill_home',
  configuredSkillHome: '/custom/skill-home', effectiveSkillHome: '/runtime/old-skill-home',
  connectionPolicy: { target: null, auto_connect: false }, mcpServerCount: 0, runtime: runtimeSnapshot(),
};
const response = {
  id: instance.id, name: instance.name, running: true, runtime: runtimeSnapshot(), connected: false,
  mcp_server_count: 0, connection_policy: { target: null, auto_connect: false },
  local_skills_root: '/next/home', configured_skill_home: '/next/home',
  effective_skill_home: instance.effectiveSkillHome, default_skill_home: instance.defaultSkillHome,
};
const edit = () => fireEvent.click(screen.getByRole('button', { name: 'Change storage location' }));
const dialog = () => within(screen.getByRole('dialog'));

describe('Advanced skill and plugin storage', () => {
  beforeEach(() => { vi.clearAllMocks(); useComputerStore.getState().reset(); useSkillStore.getState().reset(); });

  it('shows one saved path with no inline editor or save button', () => {
    render(<SkillsSettings instance={instance} />);
    expect(screen.getByText('Skill and plugin storage')).toBeInTheDocument();
    expect(screen.getByText('/custom/skill-home')).toBeInTheDocument();
    expect(within(screen.getByRole('region')).getByText('Custom directory')).toBeInTheDocument();
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Save' })).not.toBeInTheDocument();
    expect(screen.queryByText('/runtime/old-skill-home')).not.toBeInTheDocument();
  });

  it('saves one complete directory change from the editor and explains switching without migration', async () => {
    mockInvoke.mockResolvedValueOnce(response);
    render(<SkillsSettings instance={instance} />);
    edit();
    expect(dialog().getByText(/Existing files stay in the original directory/)).toBeInTheDocument();
    expect(dialog().getByText(/A running Computer keeps its current directory until restarted/)).toBeInTheDocument();
    expect(dialog().getByRole('button', { name: 'Save' })).toBeDisabled();
    fireEvent.change(dialog().getByLabelText('Custom directory path'), { target: { value: '/next/home' } });
    expect(mockInvoke).not.toHaveBeenCalled();
    fireEvent.click(dialog().getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith('update_computer_skill_home', { request: { id: instance.id, localSkillsRoot: '/next/home' } }));
    await waitFor(() => expect(screen.getByRole('dialog')).toHaveClass('ant-zoom-leave'));
    expect(mockInvoke.mock.calls.some(([command]) => /start|restart/.test(command))).toBe(false);
  });

  it('selects the default directory as a draft and discards it on cancel', async () => {
    render(<SkillsSettings instance={instance} />);
    edit();
    fireEvent.click(dialog().getByLabelText('Default directory'));
    expect(dialog().getByText(instance.defaultSkillHome!)).toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
    fireEvent.click(dialog().getByRole('button', { name: 'Cancel' }));
    edit();
    expect(dialog().getByLabelText('Custom directory')).toBeChecked();
    expect(dialog().getByLabelText('Custom directory path')).toHaveValue('/custom/skill-home');
    expect(dialog().getByRole('button', { name: 'Save' })).toBeDisabled();
  });

  it('saves the default as null and rejects a blank custom path', async () => {
    mockInvoke.mockResolvedValueOnce({ ...response, local_skills_root: null });
    render(<SkillsSettings instance={instance} />);
    edit();
    fireEvent.change(dialog().getByLabelText('Custom directory path'), { target: { value: '  ' } });
    fireEvent.click(dialog().getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Enter or choose a directory')).toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
    fireEvent.click(dialog().getByLabelText('Default directory'));
    fireEvent.click(dialog().getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith('update_computer_skill_home', { request: { id: instance.id, localSkillsRoot: null } }));
  });

  it('chooses a folder without submitting and opens only the saved directory', async () => {
    mockOpen.mockResolvedValueOnce('/chosen/home' as never);
    mockInvoke.mockResolvedValueOnce(undefined);
    render(<SkillsSettings instance={instance} />);
    fireEvent.click(screen.getByRole('button', { name: 'Open saved directory' }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith('open_configured_local_skills_root', { instanceId: instance.id }));
    edit();
    fireEvent.click(dialog().getByRole('button', { name: 'Choose Skill Home' }));
    await waitFor(() => expect(dialog().getByLabelText('Custom directory path')).toHaveValue('/chosen/home'));
    expect(mockInvoke).not.toHaveBeenCalledWith('update_computer_skill_home', expect.anything());
  });

  it('ignores a folder selection that completes after the editor is cancelled', async () => {
    let finish!: (value: string) => void;
    mockOpen.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    render(<SkillsSettings instance={instance} />);
    edit();
    fireEvent.click(dialog().getByRole('button', { name: 'Choose Skill Home' }));
    await waitFor(() => expect(finish).toBeDefined());
    fireEvent.click(dialog().getByRole('button', { name: 'Cancel' }));
    await act(async () => finish('/late/home'));
    edit();
    expect(dialog().getByLabelText('Custom directory path')).toHaveValue('/custom/skill-home');
  });

  it('renders default and legacy projections without mistaking an effective path for a saved override', () => {
    const { rerender } = render(<SkillsSettings instance={{ ...instance, localSkillsRoot: null, configuredSkillHome: instance.defaultSkillHome }} />);
    expect(within(screen.getByRole('region')).getByText('Default directory')).toBeInTheDocument();
    expect(within(screen.getByRole('region')).getByText(instance.defaultSkillHome!)).toBeInTheDocument();
    rerender(<SkillsSettings instance={{ ...instance, configuredSkillHome: undefined, defaultSkillHome: undefined, localSkillsRoot: '/saved/home' }} />);
    expect(screen.getByText('/saved/home')).toBeInTheDocument();
    expect(screen.queryByText('/runtime/old-skill-home')).not.toBeInTheDocument();
  });

  it('retains an unsaved draft while hiding the editor on navigation', async () => {
    const tree = (active: boolean) => <PageHost name="storage" active={active}><SkillsSettings instance={instance} /></PageHost>;
    const view = render(tree(true));
    edit();
    fireEvent.change(dialog().getByLabelText('Custom directory path'), { target: { value: '/draft/home' } });
    view.rerender(tree(false));
    view.rerender(tree(true));
    expect(screen.getByRole('dialog')).toHaveClass('ant-zoom-leave');
    edit();
    expect(dialog().getByLabelText('Custom directory path')).toHaveValue('/draft/home');
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it.each(['success', 'failure'] as const)('retains a delayed %s result across Computer subtree release', async (outcome) => {
    let finish!: () => void;
    mockInvoke.mockImplementationOnce(() => new Promise((resolve, reject) => { finish = () => outcome === 'success' ? resolve(response) : reject(new Error('disk unavailable')); }));
    const memory = new NavigationMemory();
    const tree = (active: boolean) => <NavigationMemoryProvider memory={memory}><NavigationScope id={active ? 'computer:a' : 'computer:b'} key={active ? 'a' : 'b'}>
      {active ? <SkillsSettings instance={useComputerStore.getState().instances.find((item) => item.id === instance.id) ?? instance} /> : <div>Other Computer</div>}
    </NavigationScope></NavigationMemoryProvider>;
    const view = render(tree(true));
    edit();
    fireEvent.change(dialog().getByLabelText('Custom directory path'), { target: { value: '/next/home' } });
    fireEvent.click(dialog().getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(finish).toBeDefined());
    view.rerender(tree(false));
    await act(async () => finish());
    view.rerender(tree(true));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    edit();
    expect(dialog().getByLabelText('Custom directory path')).toHaveValue('/next/home');
    expect(dialog().getByRole('status')).toHaveTextContent(outcome === 'success' ? 'Saved' : 'Save failed');
    if (outcome === 'failure') expect(dialog().getByRole('button', { name: 'Save' })).toBeEnabled();
  });
});
