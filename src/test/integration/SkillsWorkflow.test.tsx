import { invoke } from '@tauri-apps/api/core';
import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { describe, it, expect, beforeEach } from 'vitest';
import { Skills } from '@/components/Skills';
import { useSkillsStore, type SkillInfo } from '@/stores/skillsStore';

const mockedInvoke = vi.mocked(invoke);

const mockSkills: SkillInfo[] = [
  {
    name: 'demo-skill',
    path: '/tmp/skills/demo-skill',
    skill_md_path: '/tmp/skills/demo-skill/SKILL.md',
    has_skill_md: true,
    description: 'Demo skill description',
    source: 'local',
  },
];

describe('Skills workflow integration', () => {
  beforeEach(() => {
    useSkillsStore.getState().reset();
    mockedInvoke.mockReset();
    mockedInvoke.mockImplementation((command, args) => {
      if (command === 'list_skills') {
        return Promise.resolve(mockSkills);
      }
      if (command === 'read_skill_markdown') {
        expect(args).toEqual({ skillPath: '/tmp/skills/demo-skill' });
        return Promise.resolve('# Demo Skill\n\n- Uses `SKILL.md`');
      }
      if (command === 'open_skills_root') {
        return Promise.resolve('/tmp/skills');
      }
      if (command === 'open_skill_folder') {
        expect(args).toEqual({ skillPath: '/tmp/skills/demo-skill' });
        return Promise.resolve('/tmp/skills/demo-skill');
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });
  });

  it('loads skills through backend metadata, refreshes, opens root, and previews SKILL.md', async () => {
    render(<Skills />);

    await screen.findByText('demo-skill');
    expect(mockedInvoke).toHaveBeenCalledWith('list_skills');
    expect(screen.getByText('Demo skill description')).toBeInTheDocument();
    expect(screen.getByText('Local')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Refresh'));
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledTimes(2);
    });

    fireEvent.click(screen.getByText('Open Root Folder'));
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('open_skills_root');
    });

    fireEvent.click(screen.getByText('Folder'));
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('open_skill_folder', {
        skillPath: '/tmp/skills/demo-skill',
      });
    });

    fireEvent.click(screen.getByText('Preview'));
    await screen.findByRole('heading', { name: 'Demo Skill' });
    expect(screen.getByText(/Uses/)).toBeInTheDocument();
    expect(screen.getByText('SKILL.md')).toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith('read_skill_markdown', {
      skillPath: '/tmp/skills/demo-skill',
    });
  }, 15000);

  it('does not expose status management controls', async () => {
    render(<Skills />);

    await screen.findByText('demo-skill');

    expect(screen.queryByText('Enable')).not.toBeInTheDocument();
    expect(screen.queryByText('Disable')).not.toBeInTheDocument();
  });
});
