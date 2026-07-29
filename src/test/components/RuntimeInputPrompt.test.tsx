import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RuntimeInputPrompt } from '@/components/InputVariables/RuntimeInputPrompt';

const { getInput, addOrUpdateInput, setValue, setRuntimeValue } = vi.hoisted(() => ({
  getInput: vi.fn(),
  addOrUpdateInput: vi.fn(),
  setValue: vi.fn(),
  setRuntimeValue: vi.fn(),
}));

vi.mock('@/stores/inputStore', () => ({
  useInputStore: (selector: (state: unknown) => unknown) => selector({
    getInput,
    addOrUpdateInput,
    setValue,
    setRuntimeValue,
  }),
}));

describe('RuntimeInputPrompt', () => {
  beforeEach(() => {
    getInput.mockReset();
    addOrUpdateInput.mockReset();
    setValue.mockReset();
    setRuntimeValue.mockReset();
    getInput.mockResolvedValue({
      type: 'PromptString',
      id: 'api-key',
      label: 'API Key',
      password: true,
    });
    setValue.mockResolvedValue(undefined);
    setRuntimeValue.mockResolvedValue(false);
    addOrUpdateInput.mockResolvedValue(undefined);
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
    setValue.mockRejectedValueOnce(
      new Error('keychain /secret/path unavailable with token=private'),
    );
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

    expect(await screen.findByText(
      'The value could not be saved or the Runtime retry failed. Retry or view Runtime diagnostics or logs.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/path|token=private/)).not.toBeInTheDocument();
    expect(onSubmitted).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText('Enter value')).toBeInTheDocument();
  }, 10000);

  it('does not expose technical details when loading the Input fails', async () => {
    getInput.mockRejectedValueOnce(
      new Error('read /secret/input failed with token=private'),
    );
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
        onSubmitted={vi.fn()}
      />,
    );

    expect(await screen.findByText(
      'The Input could not be loaded. Retry or view logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/input|token=private/)).not.toBeInTheDocument();
  });

  it('creates a missing per-Computer definition before storing its value', async () => {
    getInput.mockResolvedValueOnce(null);
    const onSubmitted = vi.fn().mockResolvedValue(undefined);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_secret',
          input_id: 'OPENAI_KEY',
          env_hint: 'A2C_SMCP_OPENAI_KEY',
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

    await waitFor(() => {
      expect(addOrUpdateInput).toHaveBeenCalledWith('computer-a', {
        type: 'PromptString',
        id: 'OPENAI_KEY',
        label: 'OPENAI_KEY',
        description: 'Required secret input is unresolved',
        password: true,
      });
      expect(setValue).toHaveBeenCalledWith('computer-a', 'OPENAI_KEY', 'top-secret');
      expect(onSubmitted).toHaveBeenCalledTimes(1);
    });
  });

  it('stores a runtime-only plugin input without creating a per-Computer definition', async () => {
    getInput.mockResolvedValueOnce(null);
    setRuntimeValue.mockResolvedValueOnce(true);
    const onSubmitted = vi.fn().mockResolvedValue(undefined);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_secret',
          input_id: 'audit@acme/api-key',
          env_hint: 'A2C_SMCP_audit_acme_api_key',
          message: 'Required plugin secret is unresolved',
        }}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    fireEvent.change(await screen.findByPlaceholderText('Enter value'), {
      target: { value: 'plugin-secret' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(setRuntimeValue).toHaveBeenCalledWith(
        'computer-a',
        'audit@acme/api-key',
        'plugin-secret',
      );
      expect(addOrUpdateInput).not.toHaveBeenCalled();
      expect(setValue).not.toHaveBeenCalled();
      expect(onSubmitted).toHaveBeenCalledTimes(1);
    });
  });

  it('does not persist a Marketplace input when its runtime definition disappeared', async () => {
    getInput.mockResolvedValueOnce(null);
    setRuntimeValue.mockResolvedValueOnce(false);
    const onSubmitted = vi.fn().mockResolvedValue(undefined);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_secret',
          input_id: 'audit@acme/api-key',
          env_hint: 'A2C_SMCP_audit_acme_api_key',
          message: 'Required plugin secret is unresolved',
        }}
        allowPersistentDefinitionCreation={false}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    expect(await screen.findByPlaceholderText('Enter value')).toHaveAttribute('type', 'password');
    expect(screen.queryByRole('switch')).not.toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText('Enter value'), {
      target: { value: 'plugin-secret' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByText(
      'The value could not be saved or the Runtime retry failed. Retry or view Runtime diagnostics or logs.',
    )).toBeInTheDocument();
    expect(addOrUpdateInput).not.toHaveBeenCalled();
    expect(setValue).not.toHaveBeenCalled();
    expect(onSubmitted).not.toHaveBeenCalled();
  });

  it('keeps a Runtime retry error out of the ordinary prompt', async () => {
    const onSubmitted = vi.fn().mockRejectedValue(
      new Error('spawn /secret/runtime failed with token=private'),
    );
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

    expect(await screen.findByText(
      'The value could not be saved or the Runtime retry failed. Retry or view Runtime diagnostics or logs.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/runtime|token=private/)).not.toBeInTheDocument();
  });

  it('lets the user mark a missing value definition as secret', async () => {
    getInput.mockResolvedValueOnce(null);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_input',
          input_id: 'CUSTOM_TOKEN',
          env_hint: 'A2C_SMCP_CUSTOM_TOKEN',
          message: 'Required value input is unresolved',
        }}
        onCancel={vi.fn()}
        onSubmitted={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    const secretSwitch = await screen.findByRole('switch');
    expect(secretSwitch).not.toBeChecked();
    fireEvent.click(secretSwitch);
    const valueInput = screen.getByPlaceholderText('Enter value');
    expect(valueInput).toHaveAttribute('type', 'password');
    fireEvent.change(valueInput, { target: { value: 'secret-value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(addOrUpdateInput).toHaveBeenCalledWith(
      'computer-a',
      expect.objectContaining({ id: 'CUSTOM_TOKEN', password: true }),
    ));
  });

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
