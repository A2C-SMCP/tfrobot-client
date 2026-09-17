import { within } from '@testing-library/react';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { CredentialAccessNotice } from '@/components/CredentialAccessNotice';

describe('credential access recovery', () => {
  beforeEach(() => vi.clearAllMocks());

  it('recovers pending requests and enables only the credential explicitly selected', async () => {
    const pending = [{ id: 'opaque-request', purpose: 'oauth' }];
    vi.mocked(invoke).mockImplementation(async (command) => command === 'list_paused_credentials' ? pending : undefined);
    render(<CredentialAccessNotice />);
    fireEvent.click(await screen.findByRole('button', { name: 'Retry credential access' }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('retry_credential_access', { id: 'opaque-request' }));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Retry credential access' })).not.toBeInTheDocument());
    expect(vi.mocked(invoke).mock.calls.every(([command]) => ['list_paused_credentials', 'retry_credential_access'].includes(command))).toBe(true);
  });

  it('identifies two paused MCP credentials and retries only the selected MCP', async () => {
    vi.mocked(invoke).mockImplementation(async (command) => command === 'list_paused_credentials' ? [
      { id: 'first', purpose: 'oauth', context: { computerId: 'Work', resourceId: 'Calendar' } },
      { id: 'second', purpose: 'oauth', context: { computerId: 'Home', resourceId: 'Files' } },
    ] : undefined);
    render(<CredentialAccessNotice />);
    const title = await screen.findByText('Credential access paused: MCP sign-in — Home / Files');
    const alert = title.closest('[role="alert"]') as HTMLElement;
    fireEvent.click(within(alert).getByRole('button', { name: 'Retry credential access' }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('retry_credential_access', { id: 'second' }));
    expect(invoke).not.toHaveBeenCalledWith('retry_credential_access', { id: 'first' });
    expect(screen.getByText('Credential access paused: MCP sign-in — Work / Calendar')).toBeInTheDocument();
  });

  it('receives newly paused credentials through events and releases its listener', async () => {
    let notify = () => {};
    const stop = vi.fn();
    vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
      notify = () => handler({ event: 'credentials:access-changed', id: 0, payload: undefined as never });
      return stop;
    });
    vi.mocked(invoke).mockResolvedValueOnce([]).mockResolvedValueOnce([{ id: 'new-request', purpose: 'input' }]);
    const view = render(<CredentialAccessNotice />);
    await waitFor(() => expect(invoke).toHaveBeenCalledOnce());
    await act(async () => notify());
    expect(await screen.findByText('Credential access paused: Saved password variable')).toBeInTheDocument();
    view.unmount();
    expect(stop).toHaveBeenCalledOnce();
  });

  it('keeps the retry available when enabling access fails', async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_paused_credentials') return [{ id: 'pending', purpose: 'connection' }];
      throw new Error('Cannot enable access');
    });
    render(<CredentialAccessNotice />);
    fireEvent.click(await screen.findByRole('button', { name: 'Retry credential access' }));
    expect(await screen.findByText('Error: Cannot enable access')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('button', { name: /Retry credential access/ })).toBeEnabled());
  });
});
