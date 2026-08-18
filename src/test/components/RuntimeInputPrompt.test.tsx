import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RuntimeInputPrompt } from '@/components/InputVariables/RuntimeInputPrompt';

const { getRuntimeInput, getEntry, upsertEntry } = vi.hoisted(() => ({
  getRuntimeInput: vi.fn(),
  getEntry: vi.fn(),
  upsertEntry: vi.fn(),
}));

vi.mock('@/stores/inputStore', () => ({
  useInputStore: (selector: (state: unknown) => unknown) => selector({
    getRuntimeInput,
    getEntry,
    upsertEntry,
  }),
}));

describe('RuntimeInputPrompt', () => {
  beforeEach(() => {
    getRuntimeInput.mockReset();
    getEntry.mockReset();
    upsertEntry.mockReset();
    getRuntimeInput.mockResolvedValue({
      type: 'PromptString',
      id: 'api-key',
      label: 'API Key',
      password: true,
    });
    getEntry.mockResolvedValue(null);
    upsertEntry.mockResolvedValue(undefined);
  });

  it('creates an InputEntry and retries without creating an SDK definition', async () => {
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

    expect(await screen.findByRole('textbox', { name: 'Key' })).toBeDisabled();
    expect(screen.getByRole('textbox', { name: 'Key' })).toHaveValue('api-key');
    expect(screen.getByRole('switch', { name: 'Save as secret' })).toBeChecked();
    expect(screen.queryByText(/Keychain/i)).not.toBeInTheDocument();
    const input = await screen.findByPlaceholderText('Enter value');
    fireEvent.change(input, { target: { value: 'top-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(upsertEntry).toHaveBeenCalledWith('computer-a', 'api-key', 'top-secret', true);
      expect(onSubmitted).toHaveBeenCalledTimes(1);
    });
  });

  it('allows an empty PromptString when creating an Entry for the first time', async () => {
    const onSubmitted = vi.fn().mockResolvedValue(undefined);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_input',
          input_id: 'optional-prefix',
          env_hint: 'A2C_SMCP_optional_prefix',
          message: 'Required value input is unresolved',
        }}
        onCancel={vi.fn()}
        onSubmitted={onSubmitted}
      />,
    );

    const save = await screen.findByRole('button', { name: 'Save' });
    expect(save).not.toBeDisabled();
    fireEvent.click(save);

    await waitFor(() => {
      expect(upsertEntry).toHaveBeenCalledWith('computer-a', 'optional-prefix', '', false);
      expect(onSubmitted).toHaveBeenCalledTimes(1);
    });
  });

  it('requires an explicit replacement when Entry metadata exists but its value is missing', async () => {
    getEntry.mockResolvedValue({ key: 'api-key', secret: true });
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

    const save = await screen.findByRole('button', { name: 'Save' });
    expect(save).toBeDisabled();
    fireEvent.click(save);
    expect(upsertEntry).not.toHaveBeenCalled();
    fireEvent.change(screen.getByPlaceholderText('Leave blank to keep the current secret'), {
      target: { value: 'replacement' },
    });
    expect(save).not.toBeDisabled();
    fireEvent.click(save);

    await waitFor(() => {
      expect(upsertEntry).toHaveBeenCalledWith('computer-a', 'api-key', 'replacement', true);
      expect(onSubmitted).toHaveBeenCalledTimes(1);
    });
  });

  it('shows current PickString options and does not auto-select the first', async () => {
    getRuntimeInput.mockResolvedValue({
      type: 'PickString',
      id: 'region',
      options: [
        { label: 'China', value: 'cn' },
        { label: 'Europe', value: 'eu' },
      ],
    });
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{ code: 'missing_input', input_id: 'region', env_hint: 'A2C_SMCP_region', message: 'Choose region' }}
        onCancel={vi.fn()}
        onSubmitted={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    const select = await screen.findByRole('combobox');
    expect(select).toHaveTextContent('');
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(upsertEntry).not.toHaveBeenCalled();
    fireEvent.mouseDown(select);
    fireEvent.click(await screen.findByText('Europe'));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(upsertEntry).toHaveBeenCalledWith('computer-a', 'region', 'eu', false);
    });
  });

  it.each([
    { secret: false, entry: { key: 'region', secret: false, value: 'retired' }, errorValue: 'retired' },
    { secret: false, entry: { key: 'region', secret: false, value: '' }, errorValue: '' },
    { secret: true, entry: { key: 'region', secret: true }, errorValue: undefined },
  ])('keeps an invalid Pick Entry until a replacement option is selected (secret=$secret, value=$errorValue)', async ({ secret, entry, errorValue }) => {
    getRuntimeInput.mockResolvedValue({
      type: 'PickString',
      id: 'region',
      options: [{ label: 'China', value: 'cn' }],
    });
    getEntry.mockResolvedValue(entry);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'invalid_selection',
          input_id: 'region',
          value: errorValue,
          message: 'Stored selection is invalid',
        }}
        onCancel={vi.fn()}
        onSubmitted={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    const save = await screen.findByRole('button', { name: 'Save' });
    if (errorValue !== undefined) {
      expect(screen.queryByText('Stored secret is no longer a valid selection')).not.toBeInTheDocument();
    }
    expect(save).toBeDisabled();
    fireEvent.click(save);
    expect(upsertEntry).not.toHaveBeenCalled();

    fireEvent.mouseDown(screen.getByRole('combobox'));
    fireEvent.click(await screen.findByText('China'));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(upsertEntry).toHaveBeenCalledWith('computer-a', 'region', 'cn', secret);
    });
  });

  it('does not fabricate an Entry editor when the runtime definition is unavailable', async () => {
    getRuntimeInput.mockResolvedValue(null);
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{ code: 'missing_input', input_id: 'name', env_hint: 'A2C_SMCP_name', message: 'Missing' }}
        onCancel={vi.fn()}
        onSubmitted={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    expect(await screen.findByText(/runtime definition.*name.*unavailable/i)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Save' })).not.toBeInTheDocument();
    expect(screen.queryByText(/No saved InputEntry exists/)).not.toBeInTheDocument();
    expect(upsertEntry).not.toHaveBeenCalled();
  });

  it('uses an existing Entry secret flag and renders a redacted invalid Pick prompt', async () => {
    getRuntimeInput.mockResolvedValue({ type: 'PickString', id: 'region', options: [{ label: 'China', value: 'cn' }] });
    getEntry.mockResolvedValue({ key: 'region', secret: true });
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{ code: 'invalid_selection', input_id: 'region', message: 'Stored secret is invalid' }}
        onCancel={vi.fn()}
        onSubmitted={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    expect(await screen.findByRole('switch')).toBeChecked();
    expect(screen.getByText('Stored secret is no longer a valid selection')).toBeInTheDocument();
    expect(screen.queryByText(/retired/)).not.toBeInTheDocument();
  });

  it('shows the key and requesting MCP context', async () => {
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{
          code: 'missing_input',
          input_id: 'name',
          env_hint: 'A2C_SMCP_name',
          message: "Required value input 'name' is unresolved",
          requesting_mcp: { bundle_id: 'profile-tools', name: 'Profile Tools' },
        }}
        onCancel={vi.fn()}
        onSubmitted={vi.fn().mockResolvedValue(undefined)}
      />,
    );
    expect(await screen.findByText(/Profile Tools.*profile-tools/)).toBeInTheDocument();
    const key = screen.getByRole('textbox', { name: 'Key' });
    expect(key).toHaveValue('name');
    expect(key).toBeDisabled();
    expect(screen.getByText('Value')).toBeInTheDocument();
    expect(screen.getByRole('switch', { name: 'Save as secret' })).toBeInTheDocument();
    expect(screen.queryByText("Required value input 'name' is unresolved")).not.toBeInTheDocument();
  });

  it('does not expose storage errors or secret values', async () => {
    upsertEntry.mockRejectedValue(new Error('keychain /secret/path failed token=private'));
    render(
      <RuntimeInputPrompt
        instanceId="computer-a"
        error={{ code: 'missing_secret', input_id: 'api-key', env_hint: 'A2C_SMCP_api_key', message: 'Missing' }}
        onCancel={vi.fn()}
        onSubmitted={vi.fn().mockResolvedValue(undefined)}
      />,
    );
    fireEvent.change(await screen.findByPlaceholderText('Enter value'), { target: { value: 'top-secret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText(/The value could not be saved/)).toBeInTheDocument();
    expect(screen.queryByText(/secret\/path|token=private|top-secret/)).not.toBeInTheDocument();
  });
});
