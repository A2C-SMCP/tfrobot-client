import { invoke } from '@tauri-apps/api/core';
import { act, fireEvent, render, screen } from '../helpers/render';
import { ToolCallTest } from '@/components/DebugPanel/ToolCallTest';
import { PageActivity } from '@/components/Navigation/PageActivity';
import { useDebugStore } from '@/stores/debugStore';

it('does not execute a tool after validation outlives the active page', async () => {
  useDebugStore.getState().reset();
  vi.mocked(invoke).mockClear();
  const tree = (active: boolean) => <PageActivity active={active}><ToolCallTest instanceId="a" tool={{
    name: 'write-file', displayName: 'Write file', description: '', server: 'fs',
    inputSchema: { type: 'object', properties: { path: { type: 'string' } } },
  }} /></PageActivity>;
  const view = render(tree(true));
  fireEvent.click(screen.getByRole('button', { name: /Execute/ }));
  view.rerender(tree(false));
  await act(async () => { await Promise.resolve(); });
  view.rerender(tree(true));
  expect(invoke).not.toHaveBeenCalled();
  expect(useDebugStore.getState().activeInstanceId).toBeNull();
});
