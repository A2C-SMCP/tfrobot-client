import { fireEvent, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { RemoteControlSettings } from '@/components/ComputerSettings/RemoteControlSettings';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { render } from '@/test/helpers/render';

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
});
