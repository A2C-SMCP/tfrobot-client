import { act, render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import App from '@/App';

const managerStoreMock = vi.hoisted(() => ({
  context: {
    revision: 0,
    authState: 'signed_out' as 'signed_out' | 'onboarding_required',
    environment: null as null | 'staging',
    contextKey: null,
    user: null as null | { id: string; nickname: string; email: string; phone: string },
    account: null,
    organization: null,
    permissions: [] as string[],
  },
  restoreAttempted: false,
  applyContext: vi.fn(),
  refreshContext: vi.fn(),
  restoreSession: vi.fn().mockResolvedValue(null),
  handleAuthExpired: vi.fn().mockResolvedValue(undefined),
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

vi.mock('@/stores/managerStore', () => {
  const current = () => ({
    context: managerStoreMock.context,
    restoreAttempted: managerStoreMock.restoreAttempted,
    applyContext: managerStoreMock.applyContext,
    refreshContext: managerStoreMock.refreshContext,
    restoreSession: managerStoreMock.restoreSession,
    handleAuthExpired: managerStoreMock.handleAuthExpired,
  });
  const useManagerStore = vi.fn(current);
  Object.assign(useManagerStore, { getState: current });
  return { useManagerStore };
});

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
vi.mock('@/components/ManagerAccount/GlobalManagerAccount', () => ({
  GlobalManagerAccount: () => (
    <button aria-label="Manager Account">Global Manager Account</button>
  ),
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
vi.mock('@/components/ActivityViewer', () => ({
  ActivityViewer: () => <div>ActivityViewer</div>,
}));
vi.mock('@/components/Settings', () => ({
  Settings: () => <div>Settings</div>,
}));

describe('App', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    managerStoreMock.restoreSession.mockResolvedValue(null);
    managerStoreMock.context = {
      revision: 0,
      authState: 'signed_out',
      environment: null,
      contextKey: null,
      user: null,
      account: null,
      organization: null,
      permissions: [],
    };
    managerStoreMock.restoreAttempted = false;
    managerStoreMock.refreshContext.mockImplementation(async () => managerStoreMock.context);
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

  it('keeps the global Manager account entry visible on every page', async () => {
    render(<App />);
    expect(screen.getByRole('button', { name: 'Manager Account' })).toBeInTheDocument();

    fireEvent.click(screen.getByText('Activity'));
    await waitFor(() => expect(screen.getByText('ActivityViewer')).toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'Manager Account' })).toBeInTheDocument();
  });

  it('does not restore a previous session while onboarding guidance is active', async () => {
    managerStoreMock.context = {
      revision: 1,
      authState: 'onboarding_required',
      environment: 'staging',
      contextKey: null,
      user: { id: '99', nickname: '', email: '', phone: '' },
      account: null,
      organization: null,
      permissions: [],
    };

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
