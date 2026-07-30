import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RuntimeInputPrompt } from '@/components/InputVariables/RuntimeInputPrompt';

const { getInput, setValue } = vi.hoisted(() => ({
  getInput: vi.fn(),
  setValue: vi.fn(),
}));

vi.mock('@/stores/inputStore', () => ({
  useInputStore: (selector: (state: unknown) => unknown) => selector({ getInput, setValue }),
}));

describe('RuntimeInputPrompt', () => {
  beforeEach(() => {
    getInput.mockReset();
    setValue.mockReset();
    getInput.mockResolvedValue({
      type: 'PromptString',
      id: 'api-key',
      label: 'API Key',
      password: true,
    });
    setValue.mockResolvedValue(undefined);
  });

  it('loads the missing definition, stores the secret, and requests a retry', async () => {
    const onSubmitted = vi.fn().mockResolvedValue(undefined);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_secret',
          input_id: 'api-key',
          env_hint: 'A2C_SMCP_api_key',
          message: 'Required secret input is unresolved',
        }}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    expect(await screen.findByText('Secret required to start')).toBeInTheDocument();
    expect(screen.getByText(/A2C_SMCP_api_key/)).toBeInTheDocument();
    const input = await screen.findByPlaceholderText('Enter value');
    expect(input).toHaveAttribute('type', 'password');
    fireEvent.change(input, { target: { value: 'top-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(setValue).toHaveBeenCalledWith('computer-a', 'api-key', 'top-secret');
      expect(onSubmitted).toHaveBeenCalledTimes(1);
    });
  });

  it('never reuses a secret when the retry requests a different input', async () => {
    getInput
      .mockResolvedValueOnce({
        type: 'PromptString',
        id: 'secret-a',
        label: 'Secret A',
        password: true,
      })
      .mockResolvedValueOnce({
        type: 'PromptString',
        id: 'value-b',
        label: 'Value B',
        password: false,
      });
    const onSubmitted = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_secret',
          input_id: 'secret-a',
          env_hint: 'A2C_SMCP_secret_a',
          message: 'Secret A is missing',
        }}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    const firstInput = await screen.findByPlaceholderText('Enter value');
    fireEvent.change(firstInput, { target: { value: 'secret-a-value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(onSubmitted).toHaveBeenCalledTimes(1));

    rerender(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_input',
          input_id: 'value-b',
          env_hint: 'A2C_SMCP_value_b',
          message: 'Value B is missing',
        }}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    await waitFor(() => expect(getInput).toHaveBeenCalledWith('computer-a', 'value-b'));
    const secondInput = await screen.findByPlaceholderText('Enter value');
    expect(secondInput).toHaveAttribute('type', 'text');
    expect(secondInput).toHaveValue('');
    expect(screen.queryByDisplayValue('secret-a-value')).not.toBeInTheDocument();
  });

  it('keeps the prompt open and reports a storage failure without retrying', async () => {
    setValue.mockRejectedValueOnce(new Error('keychain unavailable'));
    const onSubmitted = vi.fn().mockResolvedValue(undefined);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_secret',
          input_id: 'api-key',
          env_hint: 'A2C_SMCP_api_key',
          message: 'Required secret input is unresolved',
        }}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    fireEvent.change(await screen.findByPlaceholderText('Enter value'), {
      target: { value: 'top-secret' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByText('Error: keychain unavailable')).toBeInTheDocument();
    expect(onSubmitted).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText('Enter value')).toBeInTheDocument();
  }, 10000);

  it('allows only one save and retry while the first retry is pending', async () => {
    let finishRetry: (() => void) | undefined;
    const onSubmitted = vi.fn(() => new Promise<void>((resolve) => {
      finishRetry = resolve;
    }));
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_secret',
          input_id: 'api-key',
          env_hint: 'A2C_SMCP_api_key',
          message: 'Required secret input is unresolved',
        }}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    const input = await screen.findByPlaceholderText('Enter value');
    fireEvent.change(input, { target: { value: 'top-secret' } });
    const save = screen.getByRole('button', { name: 'Save' });
    fireEvent.click(save);
    fireEvent.click(save);
    fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' });

    await waitFor(() => expect(onSubmitted).toHaveBeenCalledTimes(1));
    expect(setValue).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'loadingSave' })).toBeInTheDocument();

    finishRetry?.();
  });
});
