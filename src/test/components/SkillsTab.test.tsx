import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { SkillsTab } from '@/components/Computer/SkillsTab';
import styles from '@/components/Computer/SkillsTab.module.css';
import { useSkillStore } from '@/stores/skillStore';

const mockedInvoke = vi.mocked(invoke);

describe('SkillsTab', () => {
  beforeEach(() => {
    useSkillStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('groups SDK skill refs and previews SKILL.md through backend resource API', async () => {
    mockedInvoke
      .mockResolvedValueOnce([
        { name: 'local-helper', source: 'user', path: '/skills/user/local-helper', description: 'Local helper' },
        { name: 'desktop-tools:review', source: 'marketplace:tf-market', path: 'skill://marketplace/tf-market/desktop-tools/review', description: 'Marketplace helper' },
        { name: 'remote-helper', source: 'mcp:browser', path: 'skill://browser/remote-helper', description: 'Remote helper' },
      ])
      .mockResolvedValueOnce({
        name: 'local-helper',
        relPath: 'SKILL.md',
        mimeType: 'text/markdown',
        totalSize: 12,
        sha256: 'abc',
        isEntry: true,
        isText: true,
        body: '# Local Helper\n\nUse it carefully.',
      });

    render(<SkillsTab instanceId="computer-a" />);

    expect(await screen.findByText('local-helper')).toBeInTheDocument();
    expect(screen.getByText('marketplace:tf-market')).toBeInTheDocument();
    expect(screen.getByText('desktop-tools:review')).toBeInTheDocument();
    expect(screen.getByText('mcp:browser')).toBeInTheDocument();
    expect(screen.queryByText('tf-market_desktop-tools')).not.toBeInTheDocument();
    expect(screen.queryByText('mcp_browser')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /enable/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /disable/i })).not.toBeInTheDocument();
    expect(screen.getAllByText('Open Local Root')).toHaveLength(1);
    expect(screen.getByText('Open Local Root').closest('button')).toBeEnabled();

    const skillButton = screen.getByRole('button', { name: /local-helper.*Local helper/i });
    skillButton.focus();
    expect(skillButton).toHaveFocus();
    fireEvent.click(skillButton);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('get_skill', {
        instanceId: 'computer-a',
        name: 'local-helper',
        relPath: null,
      });
    });
    expect(await screen.findByRole('heading', { name: 'Local Helper' })).toBeInTheDocument();
  });

  it('refreshes active skills through the backend command for the current instance', async () => {
    mockedInvoke.mockResolvedValueOnce([]);
    mockedInvoke.mockResolvedValueOnce(undefined);
    mockedInvoke.mockResolvedValueOnce([
      { name: 'refreshed-helper', source: 'user', path: '/skills/user/refreshed-helper', description: 'Refreshed helper' },
    ]);

    render(<SkillsTab instanceId="computer-a" />);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('list_skills', { instanceId: 'computer-a' });
    });

    fireEvent.click(screen.getByText('Refresh').closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('refresh_skills', { instanceId: 'computer-a' });
    });
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('list_skills', { instanceId: 'computer-a' });
    });
    expect(await screen.findByText('refreshed-helper')).toBeInTheDocument();
  });

  it('renders long SKILL.md content inside a scrollable preview pane', async () => {
    const longBody = `# Long Helper\n\n${Array.from({ length: 80 }, (_, index) => `line ${index}`).join('\n')}`;
    mockedInvoke
      .mockResolvedValueOnce([
        { name: 'long-helper', source: 'user', path: '/skills/user/long-helper', description: 'Long helper' },
      ])
      .mockResolvedValueOnce({
        name: 'long-helper',
        relPath: 'SKILL.md',
        mimeType: 'text/markdown',
        totalSize: longBody.length,
        sha256: 'long',
        isEntry: true,
        isText: true,
        body: longBody,
      });

    render(<SkillsTab instanceId="computer-a" />);

    fireEvent.click(await screen.findByText('long-helper'));

    const heading = await screen.findByRole('heading', { name: 'Long Helper' });
    const preview = screen.getByRole('region', { name: 'Skill preview' });
    expect(preview).toHaveClass(styles.detail);
    expect(preview).toContainElement(heading);
  });

  it('filters skills without hiding the preview pane behind the list scroll', async () => {
    mockedInvoke.mockResolvedValueOnce([
      { name: 'alpha-helper', source: 'user', path: '/skills/user/alpha-helper', description: 'Alpha helper' },
      { name: 'beta-helper', source: 'marketplace:tf-market', path: 'skill://marketplace/tf-market/beta-helper', description: 'Beta helper' },
    ]);

    render(<SkillsTab instanceId="computer-a" />);

    expect(await screen.findByText('alpha-helper')).toBeInTheDocument();
    expect(screen.getByText('beta-helper')).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('Search skills'), { target: { value: 'beta' } });

    expect(screen.queryByText('alpha-helper')).not.toBeInTheDocument();
    expect(screen.getByText('beta-helper')).toBeInTheDocument();
    const preview = screen.getByRole('region', { name: 'Skill preview' });
    expect(preview).toHaveClass(styles.detail);
    expect(preview).toHaveTextContent('Select a skill to preview SKILL.md');
  });

  it('shows clear states for missing and empty SKILL.md content', async () => {
    mockedInvoke
      .mockResolvedValueOnce([
        { name: 'empty-helper', source: 'user', path: '/skills/user/empty-helper', description: 'Empty helper' },
      ])
      .mockResolvedValueOnce({
        name: 'empty-helper',
        relPath: 'SKILL.md',
        mimeType: 'text/markdown',
        totalSize: 0,
        sha256: 'empty',
        isEntry: true,
        isText: true,
        body: '',
      });

    render(<SkillsTab instanceId="computer-a" />);

    fireEvent.click(await screen.findByText('empty-helper'));

    expect(await screen.findByText('SKILL.md has no preview content')).toBeInTheDocument();
  });

  it('renders structured backend resource errors with a readable message', async () => {
    mockedInvoke
      .mockResolvedValueOnce([
        { name: 'broken-helper', source: 'user', path: '/skills/user/broken-helper', description: 'Broken helper' },
      ])
      .mockRejectedValueOnce({
        code: 'resourceNotAccessible',
        relPath: 'SKILL.md',
        reason: 'missing',
        message: 'Skill resource not accessible: reason=missing, rel_path=SKILL.md',
      });

    render(<SkillsTab instanceId="computer-a" />);

    fireEvent.click(await screen.findByText('broken-helper'));

    expect(await screen.findByText('SKILL.md unavailable')).toBeInTheDocument();
    expect(await screen.findByText('Skill resource not accessible: reason=missing, rel_path=SKILL.md')).toBeInTheDocument();
    expect(screen.queryByText('[object Object]')).not.toBeInTheDocument();
  });

  it('does not render stale skills from a different active instance', () => {
    mockedInvoke.mockReturnValue(new Promise(() => undefined) as any);
    useSkillStore.setState((state) => ({
      ...state,
      activeInstanceId: 'computer-a',
      skills: [
        { name: 'stale-helper', source: 'user', path: '/skills/user/stale-helper', description: 'A only' },
      ],
      recordsByInstanceId: {
        'computer-a': {
          skills: [
            { name: 'stale-helper', source: 'user', path: '/skills/user/stale-helper', description: 'A only' },
          ],
          selectedSkillName: null,
          selectedSkill: null,
          governance: null,
          loadingSkills: false,
          loadingSkill: false,
          loadingMarketplace: false,
          error: null,
          skillError: null,
          marketplaceError: null,
          skillsRequestId: 1,
          skillRequestId: 0,
          marketplaceRequestId: 0,
        },
      },
    }));

    render(<SkillsTab instanceId="computer-b" />);

    expect(screen.queryByText('stale-helper')).not.toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith('list_skills', { instanceId: 'computer-b' });
  });
});
