import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { describe, expect, it, vi } from 'vitest';
import { useConfigCommit } from '@/components/ComputerSettings/useConfigCommit';
import { ConfigSaveStatus } from '@/components/ComputerSettings/ConfigSaveStatus';

function Editor({ save }: { save: (value: string) => Promise<unknown> }) {
  const commit = useConfigCommit('a:name', save);
  return <><input aria-label="Edit" onChange={commit.dirty} /><button onClick={() => void commit.commit('first')}>Commit first</button><button onClick={() => void commit.commit('second')}>Commit second</button><ConfigSaveStatus state={commit.state} onRetry={commit.retry} /></>;
}
describe('complete configuration edits', () => {
  it('does not acknowledge a newer edit when an earlier save completes', async () => {
    let release!: () => void;
    const save = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    render(<Editor save={save} />);
    fireEvent.click(screen.getByText('Commit first'));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    fireEvent.change(screen.getByLabelText('Edit'), { target: { value: 'new edit' } });
    await act(async () => release());
    expect(screen.getByRole('status')).toHaveTextContent('Unsaved changes');
  });
  it('serializes submitted edits and retains the failed intent for retry', async () => {
    let release!: () => void;
    const save = vi.fn().mockImplementationOnce(() => new Promise<void>((resolve) => { release = resolve; })).mockRejectedValueOnce(new Error('disk full')).mockResolvedValueOnce(undefined);
    render(<Editor save={save} />);
    fireEvent.click(screen.getByText('Commit first'));
    fireEvent.click(screen.getByText('Commit second'));
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    await act(async () => release());
    expect(await screen.findByText('Save failed. Your edits are retained.')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Retry saving'));
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Saved'));
    expect(save.mock.calls.map(([value]) => value)).toEqual(['first', 'second', 'second']);
  });
});

describe('plugin adoption navigation', () => {
  it('shows an unconfirmed plugin consumer and only offers its owner lifecycle', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const { useConfigAdoptionStore } = await import('@/stores/configAdoptionStore');
    const { ConfigAdoptionStatus } = await import('@/components/Computer/ConfigAdoptionStatus');
    const { runtimeSnapshot } = await import('../helpers/store');
    useConfigAdoptionStore.getState().reset();
    const owner = { type: 'plugin' as const, marketplace: 'acme', plugin: 'audit', pluginId: 'audit@acme' };
    vi.mocked(invoke).mockResolvedValue({ incarnation: 1, generation: 1, state: 'unknown', changes: [], unknown_resources: ['plugin-env'], plugin_owners: { 'plugin-env': owner } });
    const open = vi.fn();
    render(<ConfigAdoptionStatus instance={{ id: 'a', name: 'A', status: 'running', connectionStatus: 'disconnected', connectionPolicy: { auto_connect: false }, mcpServerCount: 1, runtime: runtimeSnapshot() }} onOpenPlugin={open} onRestart={vi.fn()} />);
    expect(await screen.findByText('Some configuration adoption could not be confirmed')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Manage plugin audit'));
    expect(open).toHaveBeenCalledWith(owner);
    expect(screen.queryByRole('button', { name: 'Restart this MCP and adopt configuration' })).not.toBeInTheDocument();
  });
});

describe('Computer list adoption summary', () => {
  it('identifies a user MCP whose declaration adoption cannot be confirmed', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const { useConfigAdoptionStore } = await import('@/stores/configAdoptionStore');
    const { ConfigAdoptionStatus } = await import('@/components/Computer/ConfigAdoptionStatus');
    const { runtimeSnapshot } = await import('../helpers/store');
    useConfigAdoptionStore.getState().reset();
    vi.mocked(invoke).mockResolvedValue({ incarnation: 1, generation: 1, state: 'unknown', changes: [], unknown_resources: ['http-proof'] });
    render(<ConfigAdoptionStatus instance={{ id: 'a', name: 'A', status: 'running', connectionStatus: 'disconnected', connectionPolicy: { auto_connect: false }, mcpServerCount: 1, runtime: runtimeSnapshot() }} />);
    expect(await screen.findByText('http-proof')).toBeInTheDocument();
    expect(screen.getByText(/Configuration adoption for the resources below/)).toBeInTheDocument();
  });
  it.each([
    ['current', 'Config adopted'],
    ['waiting_start', 'On next start'],
    ['checking', 'Checking config'],
  ])('keeps %s visible on the list card', async (state, label) => {
    const { invoke } = await import('@tauri-apps/api/core');
    const { useConfigAdoptionStore } = await import('@/stores/configAdoptionStore');
    const { ConfigAdoptionStatus } = await import('@/components/Computer/ConfigAdoptionStatus');
    const { runtimeSnapshot } = await import('../helpers/store');
    useConfigAdoptionStore.getState().reset();
    if (state === 'checking') vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    else vi.mocked(invoke).mockResolvedValue({ incarnation: 1, generation: 1, state, changes: [], unknown_resources: [] });
    render(<ConfigAdoptionStatus compact instance={{ id: 'a', name: 'A', status: 'running', connectionStatus: 'disconnected', connectionPolicy: { auto_connect: false }, mcpServerCount: 1, runtime: runtimeSnapshot() }} />);
    expect(await screen.findByText(label)).toBeInTheDocument();
  });
});


describe('Settings adoption presentation', () => {
  it.each([
    ['waiting_start', 'Not running · Saved configuration will be used on start'],
    ['current', null],
  ])('shows %s as an ordinary status line', async (state, label) => {
    const { invoke } = await import('@tauri-apps/api/core');
    const { useConfigAdoptionStore } = await import('@/stores/configAdoptionStore');
    const { ConfigAdoptionStatus } = await import('@/components/Computer/ConfigAdoptionStatus');
    const { runtimeSnapshot } = await import('../helpers/store');
    useConfigAdoptionStore.getState().reset();
    vi.mocked(invoke).mockResolvedValue({ incarnation: 1, generation: 1, state, changes: [], unknown_resources: [] });
    render(<ConfigAdoptionStatus quiet instance={{ id: 'quiet', name: 'Quiet', status: 'running', connectionStatus: 'disconnected', connectionPolicy: { auto_connect: false }, mcpServerCount: 0, runtime: runtimeSnapshot() }} onOpenRuntime={vi.fn()} />);
    if (label) expect(await screen.findByText(label)).toBeInTheDocument();
    else {
      await waitFor(() => expect(useConfigAdoptionStore.getState().statuses.quiet?.state).toBe('current'));
      expect(screen.queryByText('This runtime has adopted the saved configuration')).not.toBeInTheDocument();
    }
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open runtime workbench' })).not.toBeInTheDocument();
  });
});
