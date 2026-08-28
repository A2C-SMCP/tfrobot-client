import { fireEvent, render, screen } from '../helpers/render';
import { McpServerList } from '@/components/McpConfig/McpServerList';
import type { McpOAuthStatus, McpServerStatus } from '@/stores/mcpStore';

function server(
  bundleId: string,
  oauth_status: McpOAuthStatus,
  plugin = false,
  oauth_interaction: McpServerStatus['oauth_interaction'] = 'interactive',
): McpServerStatus {
  const connected = oauth_status.state === 'authorized';
  return {
    bundleId,
    name: bundleId,
    activation_state: 'started',
    connection_state: connected ? 'connected' : 'authorization_required',
    running: true,
    status_message: connected ? 'connected' : 'authorization_required',
    disabled: false,
    managedBy: plugin
      ? { type: 'plugin', marketplace: 'official', plugin: 'protected-tools' }
      : { type: 'user' },
    oauth_status,
    oauth_interaction,
  };
}

function runtimeServer(
  bundleId: string,
  activation_state: McpServerStatus['activation_state'],
  connection_state: McpServerStatus['connection_state'],
): McpServerStatus {
  return {
    bundleId,
    name: bundleId,
    activation_state,
    connection_state,
    running: activation_state === 'started',
    status_message: connection_state,
    disabled: false,
    managedBy: { type: 'user' },
    oauth_status: { state: 'not_applicable' },
    oauth_interaction: 'none',
  };
}

describe('MCP OAuth runtime controls', () => {
  it('projects activation and connection into one non-contradictory user status', () => {
    render(
      <McpServerList
        servers={[
          runtimeServer('stopped', 'stopped', 'disconnected'),
          runtimeServer('disconnected', 'started', 'disconnected'),
          runtimeServer('connecting', 'started', 'connecting'),
          runtimeServer('available', 'started', 'connected'),
          runtimeServer('authorization', 'started', 'authorization_required'),
          runtimeServer('failed', 'started', 'error'),
        ]}
      />,
    );

    expect(screen.getByText('Stopped')).toBeInTheDocument();
    expect(screen.getByText('Started, not connected')).toBeInTheDocument();
    expect(screen.getByText('Connecting')).toBeInTheDocument();
    expect(screen.getByText('Available')).toBeInTheDocument();
    expect(screen.getByText('Waiting for authorization')).toBeInTheDocument();
    expect(screen.getByText('Connection failed')).toBeInTheDocument();
    expect(screen.queryByText('Running')).not.toBeInTheDocument();
    expect(screen.queryByRole('columnheader', { name: 'Message' })).not.toBeInTheDocument();
  });

  it('offers start for stopped servers and retry plus stop for failed connections', () => {
    const onStart = vi.fn().mockResolvedValue(undefined);
    const onRetry = vi.fn().mockResolvedValue(undefined);
    const onStop = vi.fn().mockResolvedValue(undefined);
    render(
      <McpServerList
        servers={[
          runtimeServer('stopped', 'stopped', 'disconnected'),
          runtimeServer('disconnected', 'started', 'disconnected'),
          runtimeServer('failed', 'started', 'error'),
          runtimeServer('available', 'started', 'connected'),
        ]}
        onStart={onStart}
        onRetry={onRetry}
        onStop={onStop}
      />,
    );

    expect(screen.getAllByTitle('Start')).toHaveLength(1);
    expect(screen.getAllByTitle('Retry connection')).toHaveLength(2);
    expect(screen.getAllByTitle('Stop')).toHaveLength(3);

    fireEvent.click(screen.getAllByTitle('Retry connection')[1]);
    fireEvent.click(screen.getAllByTitle('Stop')[0]);
    expect(onRetry).toHaveBeenCalledWith('failed', 'failed');
    expect(onStart).not.toHaveBeenCalled();
    expect(onStop).toHaveBeenCalledWith('disconnected', 'disconnected');
  });

  it('renders all six projected states for user and Plugin MCP servers', () => {
    const onAuthorize = vi.fn().mockResolvedValue(undefined);
    const onCancel = vi.fn().mockResolvedValue(undefined);
    const onClear = vi.fn().mockResolvedValue(undefined);
    render(
      <McpServerList
        servers={[
          server('plain', { state: 'not_applicable' }),
          server('new-oauth', { state: 'unauthorized' }),
          server('pending', { state: 'authorization_pending' }),
          server('ready', { state: 'authorized', scopes: ['tools.read'] }),
          server('step-up', { state: 'reauthorization_required', required_scope: 'tools.write' }),
          server('plugin-error', { state: 'error' }, true),
        ]}
        onAuthorize={onAuthorize}
        onCancelAuthorization={onCancel}
        onClearAuthorization={onClear}
      />,
    );

    expect(screen.getAllByRole('button', { name: 'Authorize' })).toHaveLength(2);
    expect(screen.getByRole('button', { name: 'Reauthorize' })).toBeInTheDocument();
    expect(screen.getByText('Additional scope required: tools.write')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    expect(screen.getByText('Authorized')).toBeInTheDocument();
    expect(screen.getByText('Authorization error')).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'Clear authorization' })).toHaveLength(2);
    expect(screen.getByText('Plugin: protected-tools')).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole('button', { name: 'Authorize' })[0]);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getAllByRole('button', { name: 'Clear authorization' })[0]);
    expect(onAuthorize).toHaveBeenCalledWith('new-oauth');
    expect(onCancel).toHaveBeenCalledWith('pending');
    expect(onClear).toHaveBeenCalledWith('ready');
  });

  it('projects both machine credential modes as status-only rows', () => {
    const onAuthorize = vi.fn().mockResolvedValue(undefined);
    render(
      <McpServerList
        servers={[
          server('client-secret', { state: 'unauthorized' }, false, 'machine'),
          server('private-key-jwt', { state: 'error' }, false, 'machine'),
          server(
            'machine-step-up',
            { state: 'reauthorization_required', required_scope: 'tools.write' },
            false,
            'machine',
          ),
        ]}
        onAuthorize={onAuthorize}
      />,
    );

    expect(screen.getByText('Not authorized')).toBeInTheDocument();
    expect(screen.getByText('Authorization error')).toBeInTheDocument();
    expect(screen.getByText('Additional scope required: tools.write')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Authorize' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Clear authorization' })).not.toBeInTheDocument();
  });
});
