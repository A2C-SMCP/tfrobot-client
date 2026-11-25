import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';
import { invoke } from '@tauri-apps/api/core';
import { act, fireEvent, render, screen, waitFor, within } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { InputVariables } from '@/components/InputVariables';
import { useUiNoticeStore } from '@/stores/uiNoticeStore';
import i18n from '@/i18n';
import { PageHost } from '@/components/Navigation/PageHost';
import { InputValueEditor } from '@/components/InputVariables/InputValueEditor';

const mockStore = {
  entries: [],
  entriesLoading: false,
  entriesLoadedInstanceId: 'computer-a',
  entriesError: null,
  fetchEntries: vi.fn().mockResolvedValue(undefined),
  upsertEntry: vi.fn().mockResolvedValue(undefined),
  deleteEntry: vi.fn().mockResolvedValue(undefined),
};

vi.mock('@/stores/inputStore', () => ({
  useInputStore: vi.fn(() => mockStore),
}));

import { useInputStore } from '@/stores/inputStore';
const mockUseInputStore = vi.mocked(useInputStore);

describe('InputVariables', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(false);
    mockUseInputStore.mockReturnValue({ ...mockStore } as any);
    useUiNoticeStore.getState().reset();
  });

  it('imports legacy variables only after the user requests it', async () => {
    vi.mocked(invoke).mockImplementation(async (command) => command === 'input_migration_pending');
    render(<InputVariables instanceId="computer-a" />);
    const button = await screen.findByRole('button', { name: 'Import older variables' });
    expect(invoke).not.toHaveBeenCalledWith('migrate_input_entries', expect.anything());
    fireEvent.click(button);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('migrate_input_entries', { instanceId: 'computer-a' }));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Import older variables' })).not.toBeInTheDocument());
    expect(mockStore.fetchEntries).toHaveBeenCalledTimes(2);
  });

  it('loads only saved InputEntries and exposes add without clear-all', () => {
    render(<InputVariables instanceId="computer-a" />);
    expect(mockStore.fetchEntries).toHaveBeenCalledWith('computer-a');
    expect(screen.getByText('Variables and credentials')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Add Input/ })).toBeInTheDocument();
    expect(screen.queryByText('Clear All Values')).not.toBeInTheDocument();
  });

  it('renders actual entries without SDK definition rows or secret plaintext', () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      entries: [
        { key: 'name', secret: false, value: 'zhangsan' },
        { key: 'api-key', secret: true },
      ],
    } as any);
    render(<InputVariables instanceId="computer-a" />);

    expect(screen.getByText('name')).toBeInTheDocument();
    expect(screen.getByText('zhangsan')).toBeInTheDocument();
    expect(screen.getByText('api-key')).toBeInTheDocument();
    expect(screen.getByText('Configured secret')).toBeInTheDocument();
    expect(screen.queryByText('PromptString')).not.toBeInTheDocument();
    expect(screen.queryByText('PickString')).not.toBeInTheDocument();
  });

  it('always allows deleting an entry because every row is saved state', async () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      entries: [{ key: 'api-key', secret: true }],
    } as any);
    render(<InputVariables instanceId="computer-a" />);

    const remove = screen.getByRole('button', { name: 'Delete input api-key' });
    expect(remove).not.toBeDisabled();
    fireEvent.click(remove);
    fireEvent.click(await screen.findByRole('button', { name: 'OK' }));
    await waitFor(() => {
      expect(mockStore.deleteEntry).toHaveBeenCalledWith('computer-a', 'api-key');
    });
  });

  it('discards a removed saved entry after an authoritative refresh', async () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, entries: [{ key: 'gone', value: 'old', secret: false }] } as any);
    const view = render(<InputVariables instanceId="computer-a" />);
    fireEvent.click(screen.getByRole('button', { name: 'Edit input gone' }));
    expect(await screen.findByRole('button', { name: 'Save' })).toBeInTheDocument();
    mockUseInputStore.mockReturnValue({ ...mockStore, entries: [] } as any);
    view.rerender(<InputVariables instanceId="computer-a" />);
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Save' })).not.toBeInTheDocument());
    expect(mockStore.upsertEntry).not.toHaveBeenCalled();
  });

  it('creates an arbitrary pre-provisioned entry', async () => {
    render(<InputVariables instanceId="computer-a" />);
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    expect(screen.getByRole('switch', { name: 'Save as secret' })).not.toBeChecked();
    expect(screen.queryByText(/Keychain/i)).not.toBeInTheDocument();
    const inputs = screen.getAllByRole('textbox');
    fireEvent.change(inputs[0], { target: { value: 'name' } });
    fireEvent.change(inputs[1], { target: { value: 'zhangsan' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockStore.upsertEntry).toHaveBeenCalledWith(
        'computer-a',
        'name',
        'zhangsan',
        false,
      );
    });
  });

  it.each([['success', false], ['success', true], ['failure', false], ['failure', true]] as const)('retains the actual InputEntry save result after delayed %s and page return (unmounted: %s)', async (outcome, released) => {
    let finish!: () => void;
    mockStore.upsertEntry.mockImplementationOnce(() => new Promise<void>((resolve, reject) => {
      finish = () => outcome === 'success' ? resolve() : reject(new Error('save unavailable'));
    }));
    const memory = new NavigationMemory();
    const tree = (active: boolean) => <NavigationMemoryProvider memory={memory}><NavigationScope id={released && !active ? 'computer:b' : 'computer:a'} key={released && !active ? 'b' : 'a'}>
      {released ? active ? <InputVariables instanceId="computer-a" /> : <div>Other Computer</div> : <PageHost name="inputs" active={active}><InputVariables instanceId="computer-a" /></PageHost>}
    </NavigationScope></NavigationMemoryProvider>;
    const view = render(tree(true));
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    const fields = screen.getAllByRole('textbox');
    fireEvent.change(fields[0], { target: { value: 'retained-input' } });
    fireEvent.change(fields[1], { target: { value: 'retained-value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(finish).toBeDefined());
    expect(fields[1]).toBeDisabled();
    view.rerender(tree(false));
    await act(async () => { finish(); });
    view.rerender(tree(true));
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent(outcome === 'success' ? /^Saved$/ : 'Save failed'));
    expect(screen.getAllByRole('button', { name: /Save$/ })).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'Retry saving' })).not.toBeInTheDocument();
    const value = screen.getAllByRole('textbox')[1];
    expect(value).toHaveValue('retained-value');
    fireEvent.change(value, { target: { value: 'new-edit' } });
    expect(screen.getByRole('status')).toHaveTextContent('Unsaved changes');
  }, 15_000);

  it.each(['success', 'failure'] as const)('reports the Input value editor result after %s and allows a new complete edit', async (outcome) => {
    let finish!: () => void;
    const submit = vi.fn(() => new Promise<void>((resolve, reject) => {
      finish = () => outcome === 'success' ? resolve() : reject(new Error('save unavailable'));
    }));
    render(<InputValueEditor inputId="value" inputs={[]} currentValue="old" onSubmit={submit} onCancel={() => {}} />);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'next' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByRole('textbox')).toBeDisabled();
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    await act(async () => { finish(); });
    expect(screen.getByRole('status')).toHaveTextContent(outcome === 'success' ? /^Saved$/ : 'Save failed');
    expect(screen.queryByRole('button', { name: 'Retry saving' })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'new-edit' } });
    expect(screen.getByRole('status')).toHaveTextContent('Unsaved changes');
  });

  it('does not close or acknowledge a replacement editor when a discarded save finishes', async () => {
    let finish!: () => void;
    mockStore.upsertEntry.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
    const memory = new NavigationMemory();
    render(<NavigationMemoryProvider memory={memory}><NavigationScope id="computer:a"><InputVariables instanceId="computer-a" /></NavigationScope></NavigationMemoryProvider>);
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    let fields = screen.getAllByRole('textbox');
    fireEvent.change(fields[0], { target: { value: 'old' } });
    fireEvent.change(fields[1], { target: { value: 'old-value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(finish).toBeDefined());
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    fields = screen.getAllByRole('textbox');
    fireEvent.change(fields[0], { target: { value: 'replacement' } });
    await act(async () => { finish(); });
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getAllByRole('textbox')[0]).toHaveValue('replacement');
    expect(screen.getByRole('status')).toHaveTextContent('Unsaved changes');
  }, 15_000);

  it('lets a user mark a new InputEntry as secret without exposing storage details', async () => {
    render(<InputVariables instanceId="computer-a" />);
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Key' }), { target: { value: 'api-key' } });
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'top-secret' } });
    fireEvent.click(screen.getByRole('switch', { name: 'Save as secret' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockStore.upsertEntry).toHaveBeenCalledWith(
        'computer-a',
        'api-key',
        'top-secret',
        true,
      );
    });
  });

  it('explains the keychain prompt in full the first time a secret is stored', async () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });
    const onOpenPermissionHelp = vi.fn();

    render(<InputVariables instanceId="computer-a" onOpenPermissionHelp={onOpenPermissionHelp} />);
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    fireEvent.click(screen.getByRole('switch', { name: 'Save as secret' }));

    const notice = await screen.findByRole('note');
    expect(notice).toHaveTextContent(i18n.t('permissions.password'));
    fireEvent.click(within(notice).getByRole('button', { name: 'Permissions & security' }));
    expect(onOpenPermissionHelp).toHaveBeenCalledTimes(1);

    fireEvent.click(within(screen.getByRole('note')).getByRole('button', { name: "Don't show again" }));
    await waitFor(() => expect(screen.queryByRole('note')).not.toBeInTheDocument());
    expect(useUiNoticeStore.getState().isDismissed('password-variable-keychain')).toBe(true);
    // The field itself keeps its one-line explanation after the bar is gone.
    expect(screen.getByText(i18n.t('permissions.password'))).toBeInTheDocument();
  });

  it('does not expose storage implementation errors when saving an entry fails', async () => {
    mockStore.upsertEntry.mockRejectedValueOnce(new Error('keychain /secret/path failed token=private'));
    render(<InputVariables instanceId="computer-a" />);
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Key' }), { target: { value: 'api-key' } });
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'top-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByText('The input could not be saved. Please try again.')).toBeInTheDocument();
    expect(screen.queryByText(/keychain|secret\/path|token=private/i)).not.toBeInTheDocument();
  });

  it('edits a secret without reading or resubmitting its plaintext', async () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      entries: [{ key: 'api-key', secret: true }],
    } as any);
    render(<InputVariables instanceId="computer-a" />);
    fireEvent.click(screen.getByRole('button', { name: 'Edit input api-key' }));
    expect(screen.queryByDisplayValue(/secret/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockStore.upsertEntry).toHaveBeenCalledWith(
        'computer-a',
        'api-key',
        undefined,
        true,
      );
    });
  });

  it('closes an editor when switching Computers', async () => {
    const view = render(<InputVariables instanceId="computer-a" />);
    fireEvent.click(screen.getByRole('button', { name: /Add Input/ }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    view.rerender(<InputVariables instanceId="computer-b" />);
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });
});
