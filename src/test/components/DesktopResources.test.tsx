import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { DesktopResources } from '@/components/DesktopResources';

const mockFetchDesktop = vi.fn().mockResolvedValue(undefined);
const mockFetchWindowDetail = vi.fn().mockResolvedValue(undefined);

const mockStore = {
  windows: [],
  loading: false,
  error: null as string | null,
  windowDetails: {} as Record<string, import('@/stores/desktopStore').WindowDetail>,
  loadingDetails: {} as Record<string, boolean>,
  detailErrors: {} as Record<string, string>,
  fetchDesktop: mockFetchDesktop,
  fetchWindowDetail: mockFetchWindowDetail,
  reset: vi.fn(),
};

vi.mock('@/stores/desktopStore', () => ({
  useDesktopStore: vi.fn(() => mockStore),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const mockUseDesktopStore = vi.mocked(
  await import('@/stores/desktopStore').then((m) => m.useDesktopStore)
);
const { invoke } = await import('@tauri-apps/api/core');
const mockInvoke = vi.mocked(invoke);

const mockWindows = [
  { uri: 'window://main', title: 'Main Window', server: 'desktop-server' },
  { uri: 'window://secondary', title: 'Secondary', server: 'desktop-server' },
];

describe('DesktopResources', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockFetchDesktop.mockResolvedValue(undefined);
    mockFetchWindowDetail.mockResolvedValue(undefined);
    mockInvoke.mockReset();
    mockUseDesktopStore.mockReturnValue({ ...mockStore } as any);
  });

  it('calls fetchDesktop on mount', () => {
    render(<DesktopResources />);
    expect(mockFetchDesktop).toHaveBeenCalled();
  });

  it('renders title and refresh button', () => {
    render(<DesktopResources />);
    expect(screen.getByText('Desktop Resources')).toBeInTheDocument();
    expect(screen.getByText('Refresh')).toBeInTheDocument();
  });

  it('renders empty state when no windows', () => {
    render(<DesktopResources />);
    expect(
      screen.getByText('No desktop windows detected'),
    ).toBeInTheDocument();
  });

  it('renders windows table', () => {
    mockUseDesktopStore.mockReturnValue({ ...mockStore, windows: mockWindows } as any);
    render(<DesktopResources />);
    expect(screen.getByText('Main Window')).toBeInTheDocument();
    expect(screen.getByText('Secondary')).toBeInTheDocument();
  });

  it('renders error alert', () => {
    mockUseDesktopStore.mockReturnValue({
      ...mockStore,
      error: 'Failed to fetch',
    } as any);
    render(<DesktopResources />);
    expect(screen.getByText('Failed to fetch')).toBeInTheDocument();
  });

  it('refreshes on button click', () => {
    render(<DesktopResources />);
    const refreshButton = screen.getByText('Refresh');
    fireEvent.click(refreshButton);
    expect(mockFetchDesktop).toHaveBeenCalledTimes(2); // Once on mount, once on click
  });

  it('shows loading spinner when loading', () => {
    mockUseDesktopStore.mockReturnValue({ ...mockStore, loading: true } as any);
    render(<DesktopResources />);
    // Ant Design Table loading state
    expect(document.querySelector('.ant-spin')).toBeInTheDocument();
  });

  it('fetches window detail when row is expanded', async () => {
    const mockDetail = {
      uri: 'window://main',
      title: 'Main Window',
      server: 'desktop-server',
      contents: [
        { type: 'text' as const, uri: 'window://main', text: 'Hello World' },
      ],
    };
    mockInvoke.mockResolvedValueOnce(mockDetail);
    mockUseDesktopStore.mockReturnValue({
      ...mockStore,
      windows: mockWindows,
    } as any);
    render(<DesktopResources />);

    // Find and click expand button (first row)
    const expandButtons = document.querySelectorAll('.ant-table-row-expand-icon');
    expect(expandButtons.length).toBeGreaterThan(0);
    fireEvent.click(expandButtons[0]);

    await waitFor(() => {
      expect(mockFetchWindowDetail).toHaveBeenCalledWith('desktop-server', 'window://main');
    });
  });

  it('renders text content in expanded row', async () => {
    const mockDetail = {
      uri: 'window://main',
      title: 'Main Window',
      server: 'desktop-server',
      contents: [
        { type: 'text' as const, uri: 'window://main', text: 'Sample text content' },
      ],
    };
    mockFetchWindowDetail.mockResolvedValueOnce(mockDetail);
    mockUseDesktopStore.mockReturnValue({
      ...mockStore,
      windows: mockWindows,
      windowDetails: { 'window://main': mockDetail },
    } as any);
    render(<DesktopResources />);

    // Expand first row
    const expandButtons = document.querySelectorAll('.ant-table-row-expand-icon');
    expect(expandButtons.length).toBeGreaterThan(0);
    fireEvent.click(expandButtons[0]);

    await waitFor(() => {
      expect(mockFetchWindowDetail).toHaveBeenCalledWith('desktop-server', 'window://main');
    });
    expect(screen.getByText('Sample text content')).toBeInTheDocument();
  });

  it('renders image content in expanded row', async () => {
    const mockDetail = {
      uri: 'window://main',
      title: 'Main Window',
      server: 'desktop-server',
      contents: [
        {
          type: 'blob' as const,
          uri: 'window://main',
          mime_type: 'image/png',
          blob: 'iVBORw0KGgo',
        },
      ],
    };
    mockFetchWindowDetail.mockResolvedValueOnce(mockDetail);
    mockUseDesktopStore.mockReturnValue({
      ...mockStore,
      windows: mockWindows,
      windowDetails: { 'window://main': mockDetail },
    } as any);
    render(<DesktopResources />);

    // Expand first row
    const expandButtons = document.querySelectorAll('.ant-table-row-expand-icon');
    expect(expandButtons.length).toBeGreaterThan(0);
    fireEvent.click(expandButtons[0]);

    await waitFor(() => {
      expect(mockFetchWindowDetail).toHaveBeenCalledWith('desktop-server', 'window://main');
    });
    // Check for image element with base64 src
    const img = document.querySelector('img[src*="base64"]');
    expect(img).toBeInTheDocument();
  });

  it('shows no content message when contents are empty', async () => {
    const mockDetail = {
      uri: 'window://main',
      title: 'Main Window',
      server: 'desktop-server',
      contents: [],
    };
    mockFetchWindowDetail.mockResolvedValueOnce(mockDetail);
    mockUseDesktopStore.mockReturnValue({
      ...mockStore,
      windows: mockWindows,
      windowDetails: { 'window://main': mockDetail },
    } as any);
    render(<DesktopResources />);

    // Expand first row
    const expandButtons = document.querySelectorAll('.ant-table-row-expand-icon');
    expect(expandButtons.length).toBeGreaterThan(0);
    fireEvent.click(expandButtons[0]);

    await waitFor(() => {
      expect(mockFetchWindowDetail).toHaveBeenCalledWith('desktop-server', 'window://main');
    });
    expect(screen.getByText('No content available')).toBeInTheDocument();
  });

  it('collapses expanded row when clicked again', async () => {
    mockFetchWindowDetail.mockResolvedValue({
      uri: 'window://main',
      title: 'Main Window',
      server: 'desktop-server',
      contents: [],
    });
    mockUseDesktopStore.mockReturnValue({
      ...mockStore,
      windows: mockWindows,
    } as any);
    render(<DesktopResources />);

    const expandButtons = document.querySelectorAll('.ant-table-row-expand-icon');
    expect(expandButtons.length).toBeGreaterThan(0);
    // First click - expand
    fireEvent.click(expandButtons[0]);

    await waitFor(() => {
      expect(mockFetchWindowDetail).toHaveBeenCalledTimes(1);
    });

    // Second click - collapse (should not trigger another fetch)
    fireEvent.click(expandButtons[0]);

    // invoke should still be called only once
    expect(mockFetchWindowDetail).toHaveBeenCalledTimes(1);
  });

  it('shows loading spinner while fetching detail', async () => {
    // Mock store with loading state for the window
    mockUseDesktopStore.mockReturnValue({
      ...mockStore,
      windows: mockWindows,
      loadingDetails: { 'window://main': true },
    } as any);
    render(<DesktopResources />);

    // Expand first row
    const expandButtons = document.querySelectorAll('.ant-table-row-expand-icon');
    expect(expandButtons.length).toBeGreaterThan(0);
    fireEvent.click(expandButtons[0]);

    // Should show loading spinner when loadingDetails contains the URI
    await waitFor(() => {
      expect(document.querySelector('.ant-spin')).toBeInTheDocument();
    });
  });
});
