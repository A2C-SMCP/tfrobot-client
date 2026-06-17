import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { RuntimeSettings } from '@/components/Settings/RuntimeSettings';
import type { AppSettings } from '@/stores/settingsStore';

const mocks = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  fetchRuntimes: vi.fn(),
  fetchDetectedPath: vi.fn(),
  settings: {
    theme: 'system',
    language: 'en',
    log_retention_days: 30,
    custom_runtime_paths: {},
    custom_path: null,
    computer_name: 'tfrobot-client',
    skills_root_dir: '~/.a2c/skills',
  } as AppSettings,
}));

vi.mock('@/stores/settingsStore', () => ({
  useSettingsStore: vi.fn(() => ({
    settings: mocks.settings,
    runtimes: [],
    detectedPath: '/usr/local/bin:/usr/bin',
    updateSettings: mocks.updateSettings,
    fetchRuntimes: mocks.fetchRuntimes,
    fetchDetectedPath: mocks.fetchDetectedPath,
  })),
}));

describe('RuntimeSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders the configured skills root directory', () => {
    render(<RuntimeSettings />);

    expect(screen.getByText('Computer Configuration')).toBeInTheDocument();
    expect(screen.getByText('Path Configuration')).toBeInTheDocument();
    expect(screen.getByText('Skills Root Directory')).toBeInTheDocument();
    expect(screen.queryByText('Computer Name')).not.toBeInTheDocument();
    expect(screen.getByDisplayValue('~/.a2c/skills')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    expect(screen.queryByText('Refresh')).not.toBeInTheDocument();
  }, 15000);

  it('cancels draft runtime setting changes without saving', async () => {
    render(<RuntimeSettings />);

    fireEvent.change(screen.getByDisplayValue('~/.a2c/skills'), {
      target: { value: '/tmp/draft-skills' },
    });

    expect(screen.getByDisplayValue('/tmp/draft-skills')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    await waitFor(() => {
      expect(screen.getByDisplayValue('~/.a2c/skills')).toBeInTheDocument();
    });
    expect(mocks.updateSettings).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
  }, 15000);

  it('saves skills root directory changes without rebuilding runtime confirmation', async () => {
    render(<RuntimeSettings />);

    fireEvent.change(screen.getByDisplayValue('~/.a2c/skills'), {
      target: { value: '/Users/test/.codex/skills' },
    });
    expect(mocks.updateSettings).not.toHaveBeenCalled();
    expect(screen.queryByText('Changing this setting rebuilds the runtime')).not.toBeInTheDocument();

    const saveButton = screen.getByRole('button', { name: 'Save' });
    expect(saveButton).not.toBeDisabled();
    fireEvent.click(saveButton);

    await waitFor(() => {
      expect(mocks.updateSettings).toHaveBeenCalledWith({
        ...mocks.settings,
        skills_root_dir: '/Users/test/.codex/skills',
      });
    });
  }, 15000);

  it('saves empty skills root directory before backend normalization', async () => {
    render(<RuntimeSettings />);

    fireEvent.change(screen.getByDisplayValue('~/.a2c/skills'), {
      target: { value: '' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mocks.updateSettings).toHaveBeenCalledWith({
        ...mocks.settings,
        skills_root_dir: '',
      });
    });
  }, 15000);

  it('does not expose a computer name editor', async () => {
    render(<RuntimeSettings />);

    expect(screen.queryByDisplayValue('tfrobot-client')).not.toBeInTheDocument();
    expect(screen.queryByText('Computer Name')).not.toBeInTheDocument();
  }, 15000);

  it('saves PATH changes only when the unified save button is clicked', async () => {
    render(<RuntimeSettings />);

    fireEvent.change(screen.getByPlaceholderText('/usr/local/bin:/usr/bin'), {
      target: { value: '/opt/homebrew/bin:/usr/bin' },
    });

    expect(mocks.updateSettings).not.toHaveBeenCalled();
    const saveButton = screen.getByRole('button', { name: 'Save' });
    expect(saveButton).not.toBeDisabled();
    fireEvent.click(saveButton);

    await waitFor(() => {
      expect(mocks.updateSettings).toHaveBeenCalledWith({
        ...mocks.settings,
        custom_path: '/opt/homebrew/bin:/usr/bin',
      });
    });
  }, 15000);
});
