import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { InputVariables } from '@/components/InputVariables';

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
    mockUseInputStore.mockReturnValue({ ...mockStore } as any);
  });

  it('loads only saved InputEntries and exposes add without clear-all', () => {
    render(<InputVariables instanceId="computer-a" />);
    expect(mockStore.fetchEntries).toHaveBeenCalledWith('computer-a');
    expect(screen.getByText('Input Entries')).toBeInTheDocument();
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
