import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { InputVariables } from '@/components/InputVariables';

const mockStore = {
  inputs: [],
  values: {},
  referenceIssues: [],
  loading: false,
  error: null,
  valuesLoading: false,
  valuesLoadedInstanceId: 'computer-a',
  valuesError: null,
  fetchInputs: vi.fn().mockResolvedValue(undefined),
  fetchValues: vi.fn().mockResolvedValue(undefined),
  fetchReferenceIssues: vi.fn().mockResolvedValue(undefined),
  addOrUpdateInput: vi.fn().mockResolvedValue(undefined),
  saveInput: vi.fn().mockResolvedValue(undefined),
  removeInput: vi.fn().mockResolvedValue(undefined),
  setValue: vi.fn().mockResolvedValue(undefined),
  removeValue: vi.fn().mockResolvedValue(undefined),
  clearValues: vi.fn().mockResolvedValue(undefined),
};

vi.mock('@/stores/inputStore', () => ({
  useInputStore: vi.fn(() => mockStore),
}));

import { useInputStore } from '@/stores/inputStore';
const mockUseInputStore = vi.mocked(useInputStore);

const mockInputs = [
  { type: 'PromptString' as const, id: 'api-key', label: 'API Key', description: 'Enter your key', password: true },
  { type: 'PickString' as const, id: 'env', label: 'Environment', options: [{ label: 'Dev', value: 'dev' }, { label: 'Prod', value: 'prod' }] },
  { type: 'Command' as const, id: 'cmd', label: 'Custom Command', command: 'echo', args: ['hello'] },
];

describe('InputVariables', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseInputStore.mockReturnValue({ ...mockStore } as any);
  });

  it('loads definitions and current value status on mount', () => {
    render(<InputVariables instanceId="computer-a" />);
    expect(mockStore.fetchInputs).toHaveBeenCalledWith('computer-a');
    expect(mockStore.fetchValues).toHaveBeenCalledWith('computer-a');
  }, 10000);

  it('renders only value-management page actions', () => {
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('Input Variables')).toBeInTheDocument();
    expect(screen.getByText('Clear All Values')).toBeInTheDocument();
    expect(screen.queryByText('Add Variable')).not.toBeInTheDocument();
  });

  it('renders inputs table with data', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: mockInputs } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('api-key')).toBeInTheDocument();
    expect(screen.getByText('env')).toBeInTheDocument();
    expect(screen.getByText('cmd')).toBeInTheDocument();
  });

  it('does not expose definition type management', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: mockInputs } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.queryByText('PromptString')).not.toBeInTheDocument();
    expect(screen.queryByText('PickString')).not.toBeInTheDocument();
    expect(screen.queryByText('Command')).not.toBeInTheDocument();
  });

  it('renders current values', () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: mockInputs,
      values: {
        'api-key': { configured: true, status: 'configured' },
        env: { configured: false, status: 'first_option' },
        cmd: { configured: false, status: 'runtime_command' },
      },
    } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('Configured secret')).toBeInTheDocument();
    expect(screen.queryByText('secret-123')).not.toBeInTheDocument();
    expect(screen.getByText('Not set; first option will be used')).toBeInTheDocument();
    expect(screen.getByText('Executed at runtime')).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /Set value for:/ })).toHaveLength(2);
    expect(screen.getByText('Secret (System Keychain)')).toBeInTheDocument();
    expect(screen.getByText('Non-secret')).toBeInTheDocument();
  });

  it('clears one saved Input value without deleting its MCP definition', async () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: [mockInputs[0]],
      values: { 'api-key': { configured: true, status: 'configured' } },
    } as any);
    render(<InputVariables instanceId="computer-a" />);

    fireEvent.click(screen.getByRole('button', { name: 'Clear the saved value for api-key' }));
    fireEvent.click(await screen.findByRole('button', { name: 'OK' }));

    await waitFor(() => {
      expect(mockStore.removeValue).toHaveBeenCalledWith('computer-a', 'api-key');
    });
    expect(mockStore.removeInput).not.toHaveBeenCalled();
  });

  it('does not expose definition edit or removal actions', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: [mockInputs[0]] } as any);
    render(<InputVariables instanceId="computer-a" />);

    expect(screen.queryByRole('button', { name: 'Edit variable api-key' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove variable api-key' })).not.toBeInTheDocument();
  });

  it('renders error alert', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, error: 'Load failed' } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('Load failed')).toBeInTheDocument();
  });

  it('does not present an unloaded value as not configured', () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: [mockInputs[0]],
      valuesLoading: true,
      valuesLoadedInstanceId: null,
    } as any);
    render(<InputVariables instanceId="computer-a" />);

    expect(screen.getByText('Loading saved value…')).toBeInTheDocument();
    expect(screen.queryByText('Not configured')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Set value for: api-key' })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Clear All Values/ })).toBeDisabled();
  });

  it('shows saved values as unavailable when their load fails', () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: [mockInputs[0]],
      valuesLoadedInstanceId: null,
      valuesError: 'Keychain unavailable',
    } as any);
    render(<InputVariables instanceId="computer-a" />);

    expect(screen.getByText('Saved value unavailable')).toBeInTheDocument();
    expect(screen.getByText('Keychain unavailable')).toBeInTheDocument();
    expect(screen.queryByText('Not configured')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Set value for: api-key' })).toBeDisabled();
  });

  it('closes an open value editor when switching Computers', async () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: [mockInputs[0]],
      values: { 'api-key': { configured: true, status: 'configured' } },
    } as any);
    const view = render(<InputVariables instanceId="computer-a" />);
    fireEvent.click(screen.getByRole('button', { name: 'Set value for: api-key' }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();

    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: [mockInputs[0]],
      valuesLoadedInstanceId: 'computer-b',
      values: { 'api-key': { configured: false, status: 'missing' } },
    } as any);
    view.rerender(<InputVariables instanceId="computer-b" />);

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(mockStore.setValue).not.toHaveBeenCalled();
  });

  it('falls back to the ID when the display label is absent', () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: [{ type: 'Command', id: 'fallback-id', command: 'echo' }],
    } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getAllByText('fallback-id').length).toBeGreaterThan(1);
  });
});
