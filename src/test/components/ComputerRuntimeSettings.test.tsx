import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { ComputerRuntimeSettings } from '@/components/Computer/ComputerRuntimeSettings';
import { runtimeSnapshot } from '../helpers/store';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';

const mockInvoke = vi.mocked(invoke);
const mockOpen = vi.mocked(open);

const instance: ComputerInstance = {
  id: 'computer-a',
  name: 'Computer A',
  status: 'running',
  connectionStatus: 'disconnected',
  localSkillsRoot: '/custom/skill-home',
  effectiveSkillHome: '/custom/skill-home',
  connectionPolicy: { target: null, auto_connect: false },
  mcpServerCount: 0,
  runtime: runtimeSnapshot(),
};

describe('ComputerRuntimeSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useComputerStore.getState().reset();
  });

  it('renders effective Skill Home and saves a custom root', async () => {
    mockInvoke.mockResolvedValueOnce({
      id: 'computer-a',
      name: 'Computer A',
      local_skills_root: '/next/skill-home',
      effective_skill_home: '/next/skill-home',
      running: true,
      runtime: runtimeSnapshot(),
      connected: false,
      mcp_server_count: 0,
      robot_binding: null,
      connection_policy: { target: null, auto_connect: false },
      connection: null,
    });

    render(<ComputerRuntimeSettings instance={instance} />);

    expect(screen.getByText('/custom/skill-home')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Instance Skill Home'), {
      target: { value: '/next/skill-home' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Rebuild this Computer?')).toBeInTheDocument();
    expect(screen.getByText(/Changing Skill Home saves a new capability governance root/)).toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Rebuild' }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('update_computer_skill_home', {
        request: {
          id: 'computer-a',
          localSkillsRoot: '/next/skill-home',
        },
      });
    });
  }, 10000);

  it('chooses a directory and restores default Skill Home', async () => {
    mockOpen.mockResolvedValueOnce('/chosen/skill-home' as any);
    mockInvoke.mockResolvedValueOnce({
      id: 'computer-a',
      name: 'Computer A',
      local_skills_root: null,
      effective_skill_home: '/default/skill_home',
      running: true,
      runtime: runtimeSnapshot(),
      connected: false,
      mcp_server_count: 0,
      robot_binding: null,
      connection_policy: { target: null, auto_connect: false },
      connection: null,
    });

    render(<ComputerRuntimeSettings instance={instance} />);

    fireEvent.click(screen.getByRole('button', { name: 'Choose Skill Home' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Instance Skill Home')).toHaveValue('/chosen/skill-home');
    });

    fireEvent.click(screen.getByRole('button', { name: 'Use Default' }));
    expect(await screen.findByText('Rebuild this Computer?')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Rebuild' }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('update_computer_skill_home', {
        request: {
          id: 'computer-a',
          localSkillsRoot: null,
        },
      });
    });
  }, 10000);
});
