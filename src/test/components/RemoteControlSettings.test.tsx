import { PageHost } from '@/components/Navigation/PageHost';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { RemoteControlSettings } from '@/components/ComputerSettings/RemoteControlSettings';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { render } from '@/test/helpers/render';
import { useUiNoticeStore } from '@/stores/uiNoticeStore';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const instance = {
  id: 'source',
  name: 'Source',
  connectionPolicy: { target: null, auto_connect: false },
  remoteControl: {
    enabled: false,
    tool_scope: { mode: 'all' },
    target_scope: { mode: 'self_only' },
  },
} as ComputerInstance;

describe('RemoteControlSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiNoticeStore.getState().reset();
    useComputerStore.setState({
      instances: [instance],
      selectedInstanceId: instance.id,
      fetchInstances: vi.fn().mockResolvedValue(undefined),
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'get_client_control_catalog') {
        return [{
          id: 'computer_list',
          group: 'computer_fleet',
          risk: 'read_only',
          target: 'discovery',
        }];
      }
      if (command === 'get_remote_control_policy') return instance.remoteControl;
      if (command === 'update_remote_control_policy') {
        return { ...instance.remoteControl, enabled: true };
      }
      throw new Error(`unexpected command: ${command}`);
    });
  });

  it('does not count a notice while loading or when loading finishes off screen', async () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });
    let finish!: (value: unknown) => void;
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) => command === 'get_remote_control_policy'
      ? new Promise((resolve) => { finish = resolve; }) : original(command, args));
    const tree = (active: boolean) => <PageHost name="remote" active={active}><RemoteControlSettings instance={instance} /></PageHost>;
    const view = render(tree(true));
    expect(useUiNoticeStore.getState().entries['remote-control-security']).toBeUndefined();
    view.rerender(tree(false));
    await act(async () => finish(instance.remoteControl));
    expect(useUiNoticeStore.getState().entries['remote-control-security']).toBeUndefined();
    vi.mocked(invoke).mockImplementation(original);
    view.rerender(tree(true));
    await screen.findByRole('note');
    expect(useUiNoticeStore.getState().entries['remote-control-security'].impressions).toBe(1);
  });

  it('does not refresh or show a completion message after the page loses ownership', async () => {
    let finish!: (policy: unknown) => void;
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) => command === 'update_remote_control_policy'
      ? new Promise((resolve) => { finish = resolve; }) : original(command, args));
    const tree = (active: boolean) => <PageHost name="remote" active={active}><RemoteControlSettings instance={instance} /></PageHost>;
    const view = render(tree(true));
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable Robot control' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(finish).toBeDefined());
    view.rerender(tree(false));
    await act(async () => { finish(instance.remoteControl); });
    expect(useComputerStore.getState().fetchInstances).not.toHaveBeenCalled();
    expect(screen.queryByText('Robot control policy saved')).not.toBeInTheDocument();
  });

  it('loads the exact catalog and persists an explicit enable decision', async () => {
    render(<RemoteControlSettings instance={instance} />);
    const toggle = await screen.findByRole('switch', { name: 'Enable Robot control' });
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      'update_remote_control_policy',
      {
        request: {
          computerId: 'source',
          policy: {
            enabled: true,
            tool_scope: { mode: 'all' },
            target_scope: { mode: 'self_only' },
          },
        },
      },
    ));
  });

  it('keeps the security notice dismissed once the user turns it off', async () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === 'update_ui_notice_state') {
        return Promise.resolve({
          schemaVersion: 1,
          entries: {
            'remote-control-security': {
              firstSeenAt: 1,
              dismissedAt: 2,
              impressions: 1,
              helpClicks: 0,
            },
          },
        });
      }
      return original(command, args);
    });
    render(<RemoteControlSettings instance={instance} />);

    const notice = await screen.findByRole('note');
    expect(notice).toHaveTextContent('This grants client-level control');
    fireEvent.click(within(notice).getByRole('button', { name: "Don't show again" }));

    await waitFor(() => {
      expect(useUiNoticeStore.getState().isDismissed('remote-control-security')).toBe(true);
    });
    expect(screen.queryByText('This grants client-level control')).not.toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith('update_ui_notice_state', expect.objectContaining({
      updates: expect.objectContaining({ 'remote-control-security': expect.objectContaining({ dismissed: true }) }),
    }));
  });
});
