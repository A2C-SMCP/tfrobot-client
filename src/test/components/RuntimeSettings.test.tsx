import { render, screen, fireEvent } from '../helpers/render';
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

    expect(screen.getByText('Skills Root Directory')).toBeInTheDocument();
    expect(screen.getByDisplayValue('~/.a2c/skills')).toBeInTheDocument();
  }, 15000);

  it('saves skills root directory changes', () => {
    render(<RuntimeSettings />);

    fireEvent.change(screen.getByDisplayValue('~/.a2c/skills'), {
      target: { value: '/Users/test/.codex/skills' },
    });

    expect(mocks.updateSettings).toHaveBeenCalledWith({
      ...mocks.settings,
      skills_root_dir: '/Users/test/.codex/skills',
    });
  }, 15000);

  it('allows clearing the skills root directory before backend normalization', () => {
    render(<RuntimeSettings />);

    fireEvent.change(screen.getByDisplayValue('~/.a2c/skills'), {
      target: { value: '' },
    });

    expect(mocks.updateSettings).toHaveBeenCalledWith({
      ...mocks.settings,
      skills_root_dir: '',
    });
  }, 15000);
});
