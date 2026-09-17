import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '../helpers/render';
import { McpServerList } from '@/components/McpConfig/McpServerList';
import type { McpServerManagedBy, McpServerStatus } from '@/stores/mcpStore';

const pluginOwner: McpServerManagedBy = { type: 'plugin', marketplace: 'official', plugin: 'desktop-tools' };
function server(bundleId: string, managedBy: McpServerManagedBy): McpServerStatus {
  return {
    bundleId, name: bundleId, managedBy,
    activation_state: 'stopped', connection_state: 'disconnected',
    running: false, status_message: 'Stopped', disabled: false,
    oauth_status: { state: 'not_applicable' }, oauth_interaction: 'none',
  };
}
const userZ = server('z-user', { type: 'user' });
const userA = server('a-user', { type: 'user' });
const pluginZ = server('z-plugin', pluginOwner);
const pluginA = server('a-plugin', pluginOwner);
const robot = server('robot', { type: 'built_in', provider: 'robot_control' });
const commandLine = server('command-line', { type: 'built_in', provider: 'command_line' });
function rowIds() {
  return screen.getAllByRole('row').flatMap((row) => {
    const id = row.getAttribute('data-row-key');
    return id === null ? [] : [id];
  });
}
function row(id: string) {
  const result = screen.getAllByRole('row').find((candidate) => candidate.getAttribute('data-row-key') === id);
  if (!result) throw new Error(`Missing row ${id}`);
  return within(result);
}

describe('MCP runtime source ordering', () => {
  it('groups sources while preserving input order within each group and leaving the shared input untouched', () => {
    const servers = [robot, pluginZ, userZ, commandLine, userA, pluginA];
    const original = [...servers];
    Object.freeze(servers);
    render(<McpServerList servers={servers} />);
    expect(rowIds()).toEqual(['z-user', 'a-user', 'z-plugin', 'a-plugin', 'robot', 'command-line']);
    expect(servers).toEqual(original);
    expect(row('z-user').getByText('User')).toBeInTheDocument();
    expect(row('z-plugin').getByText('Plugin: desktop-tools')).toBeInTheDocument();
    expect(row('robot').getByText('Built-in')).toBeInTheDocument();
  });

  it.each([
    { servers: [robot, pluginZ], expected: ['z-plugin', 'robot'] },
    { servers: [robot, userZ], expected: ['z-user', 'robot'] },
    { servers: [pluginZ, userZ], expected: ['z-user', 'z-plugin'] },
    { servers: [userZ, userA], expected: ['z-user', 'a-user'] },
    { servers: [pluginZ, pluginA], expected: ['z-plugin', 'a-plugin'] },
    { servers: [robot, commandLine], expected: ['robot', 'command-line'] },
    { servers: [], expected: [] },
  ])('handles missing sources without placeholder entries: $expected', ({ servers, expected }) => {
    render(<McpServerList servers={servers} />);
    expect(rowIds()).toEqual(expected);
  });

  it('reapplies ordering on initial load, refresh, status changes, additions and removals', () => {
    const view = render(<McpServerList servers={[]} loading />);
    view.rerender(<McpServerList servers={[robot, userA, pluginA]} />);
    expect(rowIds()).toEqual(['a-user', 'a-plugin', 'robot']);
    view.rerender(<McpServerList servers={[pluginA, robot, { ...userA }]} />);
    expect(rowIds()).toEqual(['a-user', 'a-plugin', 'robot']);
    view.rerender(<McpServerList servers={[robot, pluginA, { ...userA,
      activation_state: 'started', connection_state: 'connected', running: true }]} />);
    expect(rowIds()).toEqual(['a-user', 'a-plugin', 'robot']);
    expect(row('a-user').getByText('Available')).toBeInTheDocument();
    view.rerender(<McpServerList servers={[robot, pluginA, userA, userZ]} />);
    expect(rowIds()).toEqual(['a-user', 'z-user', 'a-plugin', 'robot']);
    view.rerender(<McpServerList servers={[robot, pluginA, userZ]} />);
    expect(rowIds()).toEqual(['z-user', 'a-plugin', 'robot']);
  });

  it('keeps names, statuses, authorization and actions bound to the correct row after sorting', () => {
    const onStart = vi.fn().mockResolvedValue(undefined);
    const onStop = vi.fn().mockResolvedValue(undefined);
    const onAuthorize = vi.fn().mockResolvedValue(undefined);
    const onOpenPlugin = vi.fn();
    render(<McpServerList servers={[robot, pluginZ, userZ, { ...userA,
      activation_state: 'started', connection_state: 'connected', running: true,
      oauth_status: { state: 'unauthorized' }, oauth_interaction: 'interactive' }]}
      onStart={onStart} onStop={onStop} onAuthorize={onAuthorize} onOpenPlugin={onOpenPlugin} />);
    expect(rowIds()).toEqual(['z-user', 'a-user', 'z-plugin', 'robot']);
    expect(row('z-user').getByText('z-user')).toBeInTheDocument();
    expect(row('z-user').getByText('Stopped')).toBeInTheDocument();
    fireEvent.click(row('z-user').getByTitle('Start'));
    fireEvent.click(row('a-user').getByTitle('Stop'));
    fireEvent.click(row('a-user').getByRole('button', { name: 'Authorize' }));
    fireEvent.click(row('z-plugin').getByRole('button'));
    expect(onStart).toHaveBeenCalledWith('z-user');
    expect(onStop).toHaveBeenCalledWith('a-user', 'a-user');
    expect(onAuthorize).toHaveBeenCalledWith('a-user');
    expect(onOpenPlugin).toHaveBeenCalledWith(pluginOwner);
    expect(row('robot').queryByRole('button')).not.toBeInTheDocument();
  });
});
