import { invoke } from '@tauri-apps/api/core';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { ResourceBrowser } from '@/components/DebugPanel/ResourceBrowser';
import { PageActivity } from '@/components/Navigation/PageActivity';
import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';
import { useDebugStore } from '@/stores/debugStore';
import { useMcpStore } from '@/stores/mcpStore';

it('keeps loaded resource pages across hidden MCP refreshes and restores their range after Computer switches', async () => {
  useDebugStore.getState().reset();
  useMcpStore.getState().reset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const { instanceId, cursor } = args as { instanceId: string; cursor?: string };
    if (command === 'get_mcp_servers') return [{
      bundleId: 'fs', name: 'filesystem', activation_state: 'started', connection_state: 'connected',
      running: true, disabled: false, status_message: 'connected', managedBy: { type: 'user' },
    }];
    if (command === 'get_debug_resources') return {
      resources: [{ server: 'filesystem', uri: `${instanceId}/${cursor ?? 'first'}`, name: `${instanceId}-${cursor ?? 'first'}` }],
      next_cursor: cursor ? undefined : 'second',
    };
    throw new Error(`Unexpected command: ${command}`);
  });
  const memory = new NavigationMemory();
  const tree = (id: string, active = true) => <NavigationMemoryProvider memory={memory}>
    <NavigationScope id={`computer:${id}`} key={id}>
      <PageActivity active={active}><ResourceBrowser instanceId={id} /></PageActivity>
    </NavigationScope>
  </NavigationMemoryProvider>;
  const view = render(tree('a'));
  expect(await screen.findByText('a-first')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: /Load More/i }));
  expect(await screen.findByText('a-second')).toBeInTheDocument();
  const requests = () => vi.mocked(invoke).mock.calls.filter(([command]) => command === 'get_debug_resources').length;
  const before = requests();
  view.rerender(tree('a', false));
  await act(async () => { await useMcpStore.getState().fetchServers('a'); });
  expect(requests()).toBe(before);
  view.rerender(tree('a'));
  await waitFor(() => expect(useMcpStore.getState().loading).toBe(false));
  expect(screen.getByText('a-second')).toBeInTheDocument();
  expect(requests()).toBe(before);
  view.rerender(tree('b'));
  expect(await screen.findByText('b-first')).toBeInTheDocument();
  view.rerender(tree('a'));
  expect(await screen.findByText('a-first')).toBeInTheDocument();
  expect(await screen.findByText('a-second')).toBeInTheDocument();
});

it('counts a resource page completed while hidden without fetching an unrequested next page', async () => {
  useDebugStore.getState().reset();
  useMcpStore.getState().reset();
  let finish!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'get_mcp_servers') return [{ bundleId: 'fs', name: 'fs', connection_state: 'connected', disabled: false }];
    const { cursor } = args as { cursor?: string };
    if (cursor === 'second') return new Promise((resolve) => { finish = resolve; });
    return { resources: [{ name: 'first', server: 'fs', uri: 'first' }], next_cursor: 'second' };
  });
  const tree = (active: boolean) => <PageActivity active={active}><ResourceBrowser instanceId="a" /></PageActivity>;
  const view = render(tree(true));
  await screen.findByRole('button', { name: /Load More/i });
  fireEvent.click(screen.getByRole('button', { name: /Load More/i }));
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(tree(false));
  await act(async () => { finish({ resources: [{ name: 'late second', server: 'fs', uri: 'second' }], next_cursor: 'third' }); });
  view.rerender(tree(true));
  await waitFor(() => expect(useMcpStore.getState().loading).toBe(false));
  expect(screen.getByText('late second')).toBeInTheDocument();
  expect(useDebugStore.getState().resourcePagesLoaded).toBe(2);
  expect(vi.mocked(invoke).mock.calls.some(([command, args]) => command === 'get_debug_resources' && (args as { cursor?: string }).cursor === 'third')).toBe(false);
});
