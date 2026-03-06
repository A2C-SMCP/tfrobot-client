import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { InputVariables } from '@/components/InputVariables';

const mockStore = {
  inputs: [],
  values: {},
  loading: false,
  error: null,
  fetchInputs: vi.fn(),
  fetchValues: vi.fn(),
  addOrUpdateInput: vi.fn(),
  removeInput: vi.fn(),
  setValue: vi.fn(),
  clearValues: vi.fn(),
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

  it('calls fetchInputs and fetchValues on mount', () => {
    render(<InputVariables />);
    expect(mockStore.fetchInputs).toHaveBeenCalled();
    expect(mockStore.fetchValues).toHaveBeenCalled();
  });

  it('renders title and action buttons', () => {
    render(<InputVariables />);
    expect(screen.getByText('Input Variables')).toBeInTheDocument();
    expect(screen.getByText('Add Variable')).toBeInTheDocument();
    expect(screen.getByText('Clear All Values')).toBeInTheDocument();
  });

  it('renders inputs table with data', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: mockInputs } as any);
    render(<InputVariables />);
    expect(screen.getByText('api-key')).toBeInTheDocument();
    expect(screen.getByText('env')).toBeInTheDocument();
    expect(screen.getByText('cmd')).toBeInTheDocument();
  });

  it('renders type tags', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, inputs: mockInputs } as any);
    render(<InputVariables />);
    expect(screen.getByText('PromptString')).toBeInTheDocument();
    expect(screen.getByText('PickString')).toBeInTheDocument();
    expect(screen.getByText('Command')).toBeInTheDocument();
  });

  it('renders current values', () => {
    mockUseInputStore.mockReturnValue({
      ...mockStore,
      inputs: mockInputs,
      values: { 'api-key': 'secret-123', env: 'dev' },
    } as any);
    render(<InputVariables />);
    expect(screen.getByText('secret-123')).toBeInTheDocument();
    expect(screen.getByText('dev')).toBeInTheDocument();
  });

  it('renders error alert', () => {
    mockUseInputStore.mockReturnValue({ ...mockStore, error: 'Load failed' } as any);
    render(<InputVariables />);
    expect(screen.getByText('Load failed')).toBeInTheDocument();
  });
});
