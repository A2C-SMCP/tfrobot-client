import { fireEvent, render, screen, waitFor, within } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { SkillsSettings } from '@/components/ComputerSettings/SkillsSettings';
import { runtimeSnapshot } from '../helpers/store';
import { useComputerStore, type ComputerInstance } from '@/stores/computerStore';
import { useSkillStore } from '@/stores/skillStore';

const mockInvoke = vi.mocked(invoke);
const mockOpen = vi.mocked(open);

const instance: ComputerInstance = {
  id: 'computer-a',
  name: 'Computer A',
  status: 'running',
  connectionStatus: 'disconnected',
  localSkillsRoot: '/custom/skill-home',
  defaultSkillHome: '/app/computer_instances/computer-a/skill_home',
  configuredSkillHome: '/custom/skill-home',
  effectiveSkillHome: '/runtime/old-skill-home',
  connectionPolicy: { target: null, auto_connect: false },
  mcpServerCount: 0,
  runtime: runtimeSnapshot(),
};

describe('SkillsSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useComputerStore.getState().reset();
    useSkillStore.getState().reset();
  });

  it('renders only the saved Skill Home and saves a custom root after confirmation', async () => {
    mockInvoke.mockResolvedValueOnce({
      id: 'computer-a',
      name: 'Computer A',
      local_skills_root: '/next/skill-home',
      effective_skill_home: '/custom/skill-home',
      running: true,
      runtime: runtimeSnapshot(),
      connected: false,
      mcp_server_count: 0,
      robot_binding: null,
      connection_policy: { target: null, auto_connect: false },
      connection: null,
    });

    render(<SkillsSettings instance={instance} />);

    expect(screen.getByText('/custom/skill-home')).toBeInTheDocument();
    expect(screen.queryByText('/runtime/old-skill-home')).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Instance Skill Home'), {
      target: { value: '/next/skill-home' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Save Skill Home?')).toBeInTheDocument();
    expect(screen.getByText(
      'This saves the capability governance root without automatically restarting the Computer.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/next time you start or restart/i)).not.toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('update_computer_skill_home', {
        request: {
          id: 'computer-a',
          localSkillsRoot: '/next/skill-home',
        },
      });
    });
  });

  it('shows the concrete default Skill Home without persisting it as an override', () => {
    render(
      <SkillsSettings
        instance={{
          ...instance,
          localSkillsRoot: null,
          configuredSkillHome: '/app/computer_instances/computer-a/skill_home',
        }}
      />,
    );

    expect(screen.getByText('/app/computer_instances/computer-a/skill_home'))
      .toBeInTheDocument();
    expect(screen.getByLabelText('Instance Skill Home')).toHaveValue('');
    expect(screen.getByLabelText('Instance Skill Home')).toHaveAttribute(
      'placeholder',
      '/app/computer_instances/computer-a/skill_home',
    );
  });

  it('uses the concrete effective path when rendering a legacy Computer projection', () => {
    render(
      <SkillsSettings
        instance={{
          ...instance,
          localSkillsRoot: null,
          defaultSkillHome: undefined,
          configuredSkillHome: undefined,
          effectiveSkillHome: '/legacy/computer-a/skill_home',
        }}
      />,
    );

    expect(screen.getByText('/legacy/computer-a/skill_home')).toBeInTheDocument();
    expect(screen.queryByText('Default instance skill_home/')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Instance Skill Home')).toHaveAttribute(
      'placeholder',
      '/legacy/computer-a/skill_home',
    );
  });

  it('does not mistake the effective runtime path for a legacy saved override', () => {
    render(
      <SkillsSettings
        instance={{
          ...instance,
          localSkillsRoot: '/saved/new-skill-home',
          defaultSkillHome: undefined,
          configuredSkillHome: undefined,
          effectiveSkillHome: '/runtime/old-skill-home',
        }}
      />,
    );

    expect(screen.getByText('/saved/new-skill-home')).toBeInTheDocument();
    expect(screen.queryByText('/runtime/old-skill-home')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Instance Skill Home')).toHaveValue(
      '/saved/new-skill-home',
    );
  });

  it('chooses and opens the instance Skill Home and restores the default', async () => {
    mockOpen.mockResolvedValueOnce('/chosen/skill-home' as never);
    mockInvoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        id: 'computer-a',
        name: 'Computer A',
        local_skills_root: null,
        effective_skill_home: '/custom/skill-home',
        running: true,
        runtime: runtimeSnapshot(),
        connected: false,
        mcp_server_count: 0,
        robot_binding: null,
        connection_policy: { target: null, auto_connect: false },
        connection: null,
      });

    render(<SkillsSettings instance={instance} />);

    fireEvent.click(screen.getByRole('button', { name: 'Choose Skill Home' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Instance Skill Home')).toHaveValue('/chosen/skill-home');
    });

    fireEvent.click(screen.getByRole('button', { name: 'Open local directory' }));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('open_configured_local_skills_root', {
        instanceId: 'computer-a',
      });
    });

    fireEvent.click(screen.getByRole('button', { name: 'Use Default' }));
    expect(await screen.findByText('Save Skill Home?')).toBeInTheDocument();
    expect(within(screen.getByRole('dialog')).getByText(
      '/app/computer_instances/computer-a/skill_home',
    )).toBeInTheDocument();
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('update_computer_skill_home', {
        request: {
          id: 'computer-a',
          localSkillsRoot: null,
        },
      });
    });
  });

  it('links to active Skills and Plugin lifecycle management', () => {
    const onNavigate = vi.fn();
    render(<SkillsSettings instance={instance} onNavigate={onNavigate} />);

    fireEvent.click(screen.getByRole('button', { name: 'View active Skills in Computer' }));
    fireEvent.click(screen.getByRole('button', { name: 'Manage Plugins and Marketplace' }));

    expect(onNavigate).toHaveBeenNthCalledWith(1, 'computer-detail:skills');
    expect(onNavigate).toHaveBeenNthCalledWith(2, 'computer-settings:plugins');
  });
});
