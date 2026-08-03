import { act, render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import App from '@/App';

const managerStoreMock = vi.hoisted(() => ({
  session: null as null,
  pendingAccountSelection: null as null,
  onboardingUserId: null as number | null,
  restoreAttempted: false,
  restoreSession: vi.fn().mockResolvedValue(null),
  handleAuthExpired: vi.fn(),
}));

const runtimeStoreMock = vi.hoisted(() => ({
  error: null as string | null,
  initialize: vi.fn().mockResolvedValue(undefined),
  dispose: vi.fn().mockResolvedValue(undefined),
  recover: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@/stores/runtimeStore', () => ({
  useRuntimeStore: (selector: (state: typeof runtimeStoreMock) => unknown) => selector(runtimeStoreMock),
}));

vi.mock('@/stores/themeStore', () => ({
  useThemeStore: vi.fn(() => ({
    resolved: 'light',
    setMode: vi.fn(),
    initFromSettings: vi.fn(),
  })),
}));

vi.mock('@/stores/managerStore', () => ({
  useManagerStore: vi.fn(() => ({
    session: managerStoreMock.session,
    pendingAccountSelection: managerStoreMock.pendingAccountSelection,
    onboardingUserId: managerStoreMock.onboardingUserId,
    restoreAttempted: managerStoreMock.restoreAttempted,
    restoreSession: managerStoreMock.restoreSession,
    handleAuthExpired: managerStoreMock.handleAuthExpired,
  })),
}));

vi.mock('@/components/Dashboard', () => ({
  Dashboard: ({ onNavigate }: { onNavigate: (key: string) => void }) => (
    <>
      <button onClick={() => onNavigate('computer-detail:overview')}>Open Computer Detail</button>
      <button onClick={() => onNavigate('computer-detail:runtime')}>Open legacy Runtime</button>
      <button onClick={() => onNavigate('computer-detail:skills')}>Open legacy Skills</button>
      <button onClick={() => onNavigate('computer-detail:resources')}>Open legacy Resources</button>
      <button onClick={() => onNavigate('computer-detail:debug')}>Open legacy Debug</button>
      <button onClick={() => onNavigate('computer-detail:logs')}>Open legacy Logs</button>
      <button onClick={() => onNavigate('computer-detail:mcp')}>Open legacy MCP</button>
      <button onClick={() => onNavigate('computer-detail:marketplace')}>
        Open legacy marketplace
      </button>
      <button onClick={() => onNavigate('computer-detail:inputs')}>Open legacy inputs</button>
      <button onClick={() => onNavigate('computer-detail:connection')}>
        Open legacy connection
      </button>
      <button onClick={() => onNavigate('computer-detail:configuration')}>Open legacy configuration</button>
      <button onClick={() => onNavigate('computer-settings:plugins:acme:audit:plugin-2')}>
        Open targeted Plugin settings
      </button>
    </>
  ),
}));
vi.mock('@/components/Computer', () => ({
  Computer: ({ initialView, initialSection }: { initialView?: 'list' | 'detail'; initialSection?: string }) => (
    <div>
      {initialView === 'detail' ? `Computer Detail View: ${initialSection}` : 'Computer List View'}
    </div>
  ),
}));
vi.mock('@/components/ManagerAccount', () => ({
  ManagerAccount: () => <div>ManagerAccount</div>,
}));
vi.mock('@/components/ComputerSettings', () => ({
  ComputerSettings: ({
    initialSection,
    targetPlugin,
  }: {
    initialSection?: string;
    targetPlugin?: { marketplace: string; plugin: string; pluginId?: string | null } | null;
  }) => (
    <div>
      Computer Settings View: {initialSection}
      {targetPlugin && `:${targetPlugin.marketplace}/${targetPlugin.plugin}/${targetPlugin.pluginId}`}
    </div>
  ),
}));
vi.mock('@/components/LogViewer', () => ({
  LogViewer: () => <div>LogViewer</div>,
}));
vi.mock('@/components/Settings', () => ({
  Settings: () => <div>Settings</div>,
}));

describe('App', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    managerStoreMock.restoreSession.mockResolvedValue(null);
    managerStoreMock.onboardingUserId = null;
    managerStoreMock.restoreAttempted = false;
    runtimeStoreMock.error = null;
    runtimeStoreMock.initialize.mockResolvedValue(undefined);
    runtimeStoreMock.dispose.mockResolvedValue(undefined);
    runtimeStoreMock.recover.mockResolvedValue(undefined);
  });

  it('shows runtime event initialization failures and retries recovery', async () => {
    runtimeStoreMock.error = 'event bridge unavailable';
    render(<App />);

    expect(screen.getByText('Runtime status updates are unavailable')).toBeInTheDocument();
    expect(screen.getByText('event bridge unavailable')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(runtimeStoreMock.recover).toHaveBeenCalledOnce());
  });

  it('restores Manager session at app startup', async () => {
    render(<App />);

    await waitFor(() => {
      expect(managerStoreMock.restoreSession).toHaveBeenCalledTimes(1);
    });
  });

  it('does not restore a previous session while onboarding guidance is active', async () => {
    managerStoreMock.onboardingUserId = 99;

    render(<App />);

    await waitFor(() => expect(runtimeStoreMock.initialize).toHaveBeenCalled());
    expect(managerStoreMock.restoreSession).not.toHaveBeenCalled();
  });

  it('returns to the Computer list from a dashboard deep link when the sidebar item is clicked', async () => {
    render(<App />);

    await act(async () => {
      fireEvent.click(screen.getByText('Open Computer Detail'));
    });
    expect(screen.getByText('Computer Detail View: top')).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByText('Computer'));
    });
    await waitFor(() => {
      expect(screen.getByText('Computer List View')).toBeInTheDocument();
    });
  });

  it('maps legacy Computer configuration routes to settings sections', async () => {
    const routes = [
      ['Open legacy MCP', 'mcp'],
      ['Open legacy marketplace', 'plugins'],
      ['Open legacy inputs', 'inputs'],
      ['Open legacy connection', 'connection'],
      ['Open legacy configuration', 'skills'],
    ] as const;

    for (const [entry, section] of routes) {
      const view = render(<App />);
      fireEvent.click(screen.getByText(entry));
      expect(screen.getByText(`Computer Settings View: ${section}`)).toBeInTheDocument();
      view.unmount();
    }
  });

  it('maps legacy runtime routes to workbench sections without preserving tabs', () => {
    const routes = [
      ['Open Computer Detail', 'top'],
      ['Open legacy Runtime', 'top'],
      ['Open legacy Skills', 'skills'],
      ['Open legacy Resources', 'resources'],
      ['Open legacy Debug', 'debug'],
      ['Open legacy Logs', 'logs'],
    ] as const;

    for (const [entry, section] of routes) {
      const view = render(<App />);
      fireEvent.click(screen.getByText(entry));
      expect(screen.getByText(`Computer Detail View: ${section}`)).toBeInTheDocument();
      view.unmount();
    }
  });

  it('preserves a targeted Plugin destination in Computer settings navigation', () => {
    render(<App />);
    fireEvent.click(screen.getByText('Open targeted Plugin settings'));
    expect(screen.getByText(
      'Computer Settings View: plugins:acme/audit/plugin-2',
    )).toBeInTheDocument();
  });
});
