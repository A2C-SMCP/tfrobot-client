import { invoke } from '@tauri-apps/api/core';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { ToolBrowser } from '@/components/DebugPanel/ToolBrowser';
import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';
import { useDebugStore, type ToolInfo } from '@/stores/debugStore';

vi.mock('@/components/DebugPanel/ToolCallTest', () => ({ ToolCallTest: () => null }));

it('restores a Computer filter after another Computer has populated the shared debug store', async () => {
  useDebugStore.getState().reset();
  const tools = (id: string): ToolInfo[] => [{ name: id, displayName: id, description: '', inputSchema: {}, server: `server-${id}` }];
  let aVisits = 0;
  let finish!: (tools: ToolInfo[]) => void;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command !== 'get_available_tools') throw new Error(`Unexpected command: ${command}`);
    const id = (args as { instanceId: string }).instanceId;
    if (id === 'a' && ++aVisits === 2) return new Promise<ToolInfo[]>((resolve) => { finish = resolve; });
    return tools(id);
  });
  const memory = new NavigationMemory();
  const tree = (id: string) => <NavigationMemoryProvider memory={memory}>
    <NavigationScope id={`computer:${id}`} key={id}><ToolBrowser instanceId={id} /></NavigationScope>
  </NavigationMemoryProvider>;
  const view = render(tree('a'));
  fireEvent.mouseDown(await screen.findByRole('combobox'));
  const options = await screen.findAllByText('server-a');
  fireEvent.click(options[options.length - 1]);
  await waitFor(() => expect(view.container.querySelector('.ant-select-selection-item')).toHaveTextContent('server-a'));
  view.rerender(tree('b'));
  await waitFor(() => expect(useDebugStore.getState().toolsLoadedInstanceId).toBe('b'));
  view.rerender(tree('a'));
  await waitFor(() => expect(finish).toBeDefined());
  await act(async () => { finish(tools('a')); });
  await waitFor(() => expect(view.container.querySelector('.ant-select-selection-item')).toHaveTextContent('server-a'));
});
