import { act, render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import App from '@/App';

vi.mock('@/stores/themeStore', () => ({
  useThemeStore: vi.fn(() => ({
    resolved: 'light',
    setMode: vi.fn(),
    initFromSettings: vi.fn(),
  })),
}));

vi.mock('@/components/Dashboard', () => ({
  Dashboard: ({ onNavigate }: { onNavigate: (key: string) => void }) => (
    <button onClick={() => onNavigate('computer-detail:connection')}>Open Computer Detail</button>
  ),
}));
vi.mock('@/components/Computer', () => ({
  Computer: ({ initialView, initialTab }: { initialView?: 'list' | 'detail'; initialTab?: string }) => (
    <div>
      {initialView === 'detail' ? `Computer Detail View: ${initialTab}` : 'Computer List View'}
    </div>
  ),
}));
vi.mock('@/components/ManagerAccount', () => ({
  ManagerAccount: () => <div>ManagerAccount</div>,
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
  });

  it('returns to the Computer list from a dashboard deep link when the sidebar item is clicked', async () => {
    render(<App />);

    await act(async () => {
      fireEvent.click(screen.getByText('Open Computer Detail'));
    });
    expect(screen.getByText('Computer Detail View: connection')).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByText('Computer'));
    });
    await waitFor(() => {
      expect(screen.getByText('Computer List View')).toBeInTheDocument();
    });
  });
});
