import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { InputVariables } from '@/components/InputVariables';
import { InputForm } from '@/components/InputVariables/InputForm';

const mockStore = {
  inputs: [],
  values: {},
  referenceIssues: [],
  loading: false,
  error: null,
  fetchInputs: vi.fn().mockResolvedValue(undefined),
  fetchValues: vi.fn().mockResolvedValue(undefined),
  fetchReferenceIssues: vi.fn().mockResolvedValue(undefined),
  addOrUpdateInput: vi.fn().mockResolvedValue(undefined),
  saveInput: vi.fn().mockResolvedValue(undefined),
  removeInput: vi.fn().mockResolvedValue(undefined),
  setValue: vi.fn().mockResolvedValue(undefined),
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

  it('renders title and action buttons', () => {
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('Input Variables')).toBeInTheDocument();
    expect(screen.getByText('Add Variable')).toBeInTheDocument();
    expect(screen.getByText('Clear All Values')).toBeInTheDocument();
  });

  it('renders inputs table with data', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: mockInputs } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('api-key')).toBeInTheDocument();
    expect(screen.getByText('env')).toBeInTheDocument();
    expect(screen.getByText('cmd')).toBeInTheDocument();
  });

  it('renders type tags', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: mockInputs } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('PromptString')).toBeInTheDocument();
    expect(screen.getByText('PickString')).toBeInTheDocument();
    expect(screen.getByText('Command')).toBeInTheDocument();
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
  });

  it('gives every row action an accessible name', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: [mockInputs[0]] } as any);
    render(<InputVariables instanceId="computer-a" />);

    expect(screen.getByRole('button', { name: 'Edit variable api-key' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Remove variable api-key' })).toBeVisible();
  });

  it('renders error alert', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, error: 'Load failed' } as any);
    render(<InputVariables instanceId="computer-a" />);
    expect(screen.getByText('Load failed')).toBeInTheDocument();
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

describe('InputForm runtime-value semantics', () => {
  it('submits a Prompt definition with its default separate from the actual value store', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<InputForm onSubmit={onSubmit} onCancel={() => {}} />);

    fireEvent.change(screen.getByRole('textbox', { name: 'Variable ID' }), {
      target: { value: 'token' },
    });
    fireEvent.change(screen.getByRole('textbox', { name: 'Default Value' }), {
      target: { value: 'fallback' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledWith({
      type: 'PromptString',
      id: 'token',
      label: undefined,
      description: undefined,
      default: 'fallback',
      password: undefined,
    }));
  }, 15_000);

  it('never asks for or serializes a plaintext default for password definitions', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <InputForm
        initialValues={{ type: 'PromptString', id: 'token', password: true }}
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    expect(screen.queryByLabelText('Default Value')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledWith({
      type: 'PromptString',
      id: 'token',
      label: undefined,
      description: undefined,
      default: undefined,
      password: true,
    }));
  });

  it('requires at least one PickString option', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <InputForm
        initialValues={{ type: 'PickString', id: 'region', options: [] }}
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Add at least one option.')).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  }, 10000);
});
