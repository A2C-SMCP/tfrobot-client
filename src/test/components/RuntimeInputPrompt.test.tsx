import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RuntimeInputPrompt } from '@/components/InputVariables/RuntimeInputPrompt';
import { useRuntimeInputStore, type RuntimeInputRequest } from '@/stores/runtimeInputStore';
import { useInputStore } from '@/stores/inputStore';
import { completeRuntimeInputRequest } from '@/services/runtimeInputBridge';

vi.mock('@/services/runtimeInputBridge', () => ({
  completeRuntimeInputRequest: vi.fn(),
  RuntimeInputCompletionError: class RuntimeInputCompletionError extends Error {
    constructor(message: string, readonly terminal: boolean) {
      super(message);
    }
  },
}));

const complete = vi.mocked(completeRuntimeInputRequest);

function enqueue(request: Partial<RuntimeInputRequest> = {}) {
  useRuntimeInputStore.getState().enqueue({
    requestId: 'request-1',
    instanceId: 'computer-a',
    definition: {
      type: 'PromptString',
      id: 'name',
      description: 'Your name',
    },
    reason: 'missing',
    secret: false,
    ...request,
  });
}

describe('RuntimeInputPrompt', () => {
  beforeEach(() => {
    useRuntimeInputStore.getState().reset();
    useInputStore.setState({ activeInstanceId: null });
    complete.mockReset();
    complete.mockResolvedValue(undefined);
  });

  it('uses a PromptString default only as editable prefill and confirms the edited value', async () => {
    enqueue({
      definition: {
        type: 'PromptString',
        id: 'name',
        description: 'Your name',
        default: 'Ada',
      },
    });
    render(<RuntimeInputPrompt />);

    const input = screen.getByRole('textbox', { name: 'Value' });
    expect(input).toHaveValue('Ada');
    fireEvent.change(input, { target: { value: 'Grace' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(complete).toHaveBeenCalledWith('request-1', {
        status: 'confirmed',
        value: 'Grace',
      });
      expect(useRuntimeInputStore.getState().requests).toEqual([]);
    });
  });

  it('preselects an explicit PickString default without choosing the first option implicitly', async () => {
    enqueue({
      definition: {
        type: 'PickString',
        id: 'region',
        options: [
          { label: 'China', value: 'cn' },
          { label: 'Europe', value: 'eu' },
        ],
        default: 'eu',
      },
    });
    render(<RuntimeInputPrompt />);

    expect(screen.getByText('Europe')).toBeInTheDocument();
    expect(screen.queryByRole('switch', { name: 'Save as secret' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(complete).toHaveBeenCalledWith('request-1', {
        status: 'confirmed',
        value: 'eu',
      });
    });
  });

  it('requires a legal PickString selection when no default exists', async () => {
    enqueue({
      definition: {
        type: 'PickString',
        id: 'region',
        options: [
          { label: 'China', value: 'cn' },
          { label: 'Europe', value: 'eu' },
        ],
      },
    });
    render(<RuntimeInputPrompt />);

    const select = screen.getByRole('combobox');
    expect(screen.queryByRole('switch', { name: 'Save as secret' })).not.toBeInTheDocument();
    expect(select).toHaveTextContent('');
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    fireEvent.mouseDown(select);
    fireEvent.click(await screen.findByText('China'));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(complete).toHaveBeenCalledWith('request-1', {
        status: 'confirmed',
        value: 'cn',
      });
    });
  });

  it('hides the secret control when an existing secret PickString needs reconfirmation', () => {
    enqueue({
      definition: {
        type: 'PickString',
        id: 'region',
        options: [{ label: 'China', value: 'cn' }],
        default: 'cn',
      },
      reason: 'invalid_selection',
      secret: true,
    });
    render(<RuntimeInputPrompt />);

    expect(screen.getByText('China')).toBeInTheDocument();
    expect(screen.queryByRole('switch', { name: 'Save as secret' })).not.toBeInTheDocument();
  });

  it('forces password PromptString values to remain secret', () => {
    enqueue({
      definition: {
        type: 'PromptString',
        id: 'api-key',
        password: true,
        default: 'plaintext-must-not-prefill',
      },
      secret: true,
    });
    render(<RuntimeInputPrompt />);

    const secret = screen.getByRole('switch', { name: 'Save as secret' });
    expect(secret).toBeChecked();
    expect(secret).toBeDisabled();
    expect(screen.getByLabelText('Value')).toHaveValue('');
  });

  it('cancels the original resolver request without submitting a value', async () => {
    enqueue();
    render(<RuntimeInputPrompt />);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(complete).toHaveBeenCalledWith('request-1', { status: 'cancelled' });
      expect(useRuntimeInputStore.getState().requests).toEqual([]);
    });
  });

  it('closes a terminally failed request and reports a sanitized global failure', async () => {
    const { RuntimeInputCompletionError } = await import('@/services/runtimeInputBridge');
    complete.mockRejectedValue(
      new RuntimeInputCompletionError('keychain /secret/path token=private', true),
    );
    enqueue({ secret: true });
    render(<RuntimeInputPrompt />);
    fireEvent.change(screen.getByLabelText('Value'), {
      target: { value: 'top-secret' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(useRuntimeInputStore.getState().requests).toEqual([]);
      expect(useRuntimeInputStore.getState().completionFailed).toBe(true);
    });
    expect(screen.queryByText(/secret\/path|token=private|top-secret/)).not.toBeInTheDocument();
  });

  it('drops an inactive cancelled request instead of leaving a stuck modal', async () => {
    const { RuntimeInputCompletionError } = await import('@/services/runtimeInputBridge');
    complete.mockRejectedValue(new RuntimeInputCompletionError('no longer active', true));
    enqueue();
    render(<RuntimeInputPrompt />);

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(useRuntimeInputStore.getState().requests).toEqual([]);
      expect(useRuntimeInputStore.getState().completionFailed).toBe(true);
    });
  });

  it('keeps a request visible when cancellation fails before native admission', async () => {
    const { RuntimeInputCompletionError } = await import('@/services/runtimeInputBridge');
    complete.mockRejectedValue(new RuntimeInputCompletionError('transport unavailable', false));
    enqueue();
    render(<RuntimeInputPrompt />);

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(await screen.findByText('The value could not be saved. Retry or view Runtime diagnostics or logs.'))
      .toBeInTheDocument();
    expect(useRuntimeInputStore.getState().requests).toHaveLength(1);
  });
});
