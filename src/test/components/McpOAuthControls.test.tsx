import { fireEvent, render, screen } from '../helpers/render';
import { McpServerList } from '@/components/McpConfig/McpServerList';
import type { McpOAuthStatus, McpServerStatus } from '@/stores/mcpStore';

function server(
  bundleId: string,
  oauth_status: McpOAuthStatus,
  plugin = false,
  oauth_interaction: McpServerStatus['oauth_interaction'] = 'interactive',
): McpServerStatus {
  return {
    bundleId,
    name: bundleId,
    running: false,
    status_message: 'stopped',
    disabled: false,
    managedBy: plugin
      ? { type: 'plugin', marketplace: 'official', plugin: 'protected-tools' }
      : { type: 'user' },
    oauth_status,
    oauth_interaction,
  };
}

describe('MCP OAuth runtime controls', () => {
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
