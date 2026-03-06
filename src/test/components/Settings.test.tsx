import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { Settings } from '@/components/Settings';

vi.mock('@/stores/settingsStore', () => ({
  useSettingsStore: vi.fn(() => ({
    settings: null,
    runtimes: [],
    appInfo: null,
    loading: false,
    error: null,
    fetchSettings: vi.fn(),
    updateSettings: vi.fn(),
    fetchRuntimes: vi.fn(),
    fetchAppInfo: vi.fn(),
  })),
}));

vi.mock('@/stores/themeStore', () => ({
  useThemeStore: vi.fn(() => ({
    mode: 'system',
    resolved: 'light',
    setMode: vi.fn(),
  })),
}));

vi.mock('@/stores/logStore', () => ({
  useLogStore: vi.fn(() => ({
    clearLogs: vi.fn(),
  })),
}));

vi.mock('@/stores/mcpStore', () => ({
  useMcpStore: vi.fn(() => ({
    exportConfig: vi.fn(),
  })),
}));

// Mock sub-components to isolate Settings container
vi.mock('@/components/Settings/AppearanceSettings', () => ({
  AppearanceSettings: () => <div data-testid="appearance-settings">AppearanceSettings</div>,
}));
vi.mock('@/components/Settings/RuntimeSettings', () => ({
  RuntimeSettings: () => <div data-testid="runtime-settings">RuntimeSettings</div>,
}));
vi.mock('@/components/Settings/DataSettings', () => ({
  DataSettings: () => <div data-testid="data-settings">DataSettings</div>,
}));
vi.mock('@/components/Settings/AboutSection', () => ({
  AboutSection: () => <div data-testid="about-section">AboutSection</div>,
}));

describe('Settings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders title', () => {
    render(<Settings />);
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });

  it('renders all setting tabs', () => {
    render(<Settings />);
    expect(screen.getByText('Appearance')).toBeInTheDocument();
    expect(screen.getByText('Runtime')).toBeInTheDocument();
    expect(screen.getByText('Data')).toBeInTheDocument();
    expect(screen.getByText('About')).toBeInTheDocument();
  });

  it('renders AppearanceSettings as default tab', () => {
    render(<Settings />);
    expect(screen.getByTestId('appearance-settings')).toBeInTheDocument();
  });
});
