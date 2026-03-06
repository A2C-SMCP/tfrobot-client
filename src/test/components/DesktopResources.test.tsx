import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { DesktopResources } from '@/components/DesktopResources';

const mockStore = {
  windows: [],
  loading: false,
  error: null,
  fetchDesktop: vi.fn(),
};

vi.mock('@/stores/desktopStore', () => ({
  useDesktopStore: vi.fn(() => mockStore),
}));

import { useDesktopStore } from '@/stores/desktopStore';
const mockUseDesktopStore = vi.mocked(useDesktopStore);

const mockWindows = [
  { uri: 'screen://main', title: 'Main Window', server: 'desktop-server' },
  { uri: 'screen://secondary', title: 'Secondary', server: 'desktop-server' },
];

describe('DesktopResources', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseDesktopStore.mockReturnValue({ ...mockStore } as any);
  });

  it('calls fetchDesktop on mount', () => {
    render(<DesktopResources />);
    expect(mockStore.fetchDesktop).toHaveBeenCalled();
  });

  it('renders title and refresh button', () => {
    render(<DesktopResources />);
    expect(screen.getByText('Desktop Resources')).toBeInTheDocument();
    expect(screen.getByText('Refresh')).toBeInTheDocument();
  });

  it('renders empty state when no windows', () => {
    render(<DesktopResources />);
    expect(screen.getByText('No desktop windows detected')).toBeInTheDocument();
  });

  it('renders windows table', () => {
    mockUseDesktopStore.mockReturnValue({ ...mockStore, windows: mockWindows } as any);
    render(<DesktopResources />);
    expect(screen.getByText('Main Window')).toBeInTheDocument();
    expect(screen.getByText('Secondary')).toBeInTheDocument();
  });

  it('renders error alert', () => {
    mockUseDesktopStore.mockReturnValue({ ...mockStore, error: 'Failed to fetch' } as any);
    render(<DesktopResources />);
    expect(screen.getByText('Failed to fetch')).toBeInTheDocument();
  });
});
