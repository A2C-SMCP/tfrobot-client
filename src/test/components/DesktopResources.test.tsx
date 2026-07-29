import { invoke } from '@tauri-apps/api/core';
import { DesktopResources } from '@/components/DesktopResources';
import {
  desktopRuntimeKey,
  desktopWindowKey,
  selectDesktopInstance,
  useDesktopStore,
  type DesktopInstanceState,
  type DesktopWindow,
} from '@/stores/desktopStore';
import { runtimeSnapshot } from '../helpers/store';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';

const mockedInvoke = vi.mocked(invoke);
const activeRuntime = runtimeSnapshot({
  lifecycle: 'started',
  mcp_servers: 1,
  active_mcp_servers: 1,
});
const activeRuntimeKey = desktopRuntimeKey(activeRuntime);
const windows: DesktopWindow[] = [
  {
    bundleId: 'desktop-bundle',
    uri: 'window://main',
    title: 'Main Window',
    server: 'Desktop MCP',
    mime_type: 'text/plain',
  },
  {
    bundleId: 'desktop-bundle',
    uri: 'window://secondary',
    title: 'Secondary',
    server: 'Desktop MCP',
    mime_type: 'image/png',
  },
];

function setDesktopState(instanceId: string, update: Partial<DesktopInstanceState>) {
  act(() => {
    useDesktopStore.setState((state) => ({
      instances: {
        ...state.instances,
        [instanceId]: {
          ...selectDesktopInstance(state, instanceId),
          runtimeKey: activeRuntimeKey,
          ...update,
        },
      },
    }));
  });
}

function renderDesktop(
  props: Partial<React.ComponentProps<typeof DesktopResources>> = {},
) {
  return render(
    <DesktopResources
      instanceId="computer-a"
      runtime={activeRuntime}
      {...props}
    />,
  );
}

function expandSection() {
  fireEvent.click(screen.getByText('Desktop Resources'));
}

function firstRowExpandButton(): Element {
  const button = document.querySelector('.ant-table-row-expand-icon');
  expect(button).toBeInTheDocument();
  return button!;
}

describe('DesktopResources', () => {
  beforeEach(() => {
    useDesktopStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('is collapsed by default and performs zero resource requests on render', () => {
    renderDesktop();

    expect(screen.getByText('Desktop Resources')).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Load resources' })).not.toBeInTheDocument();
  });

  it('still performs zero requests when the section is expanded', () => {
    renderDesktop();

    expandSection();

    expect(screen.getByRole('button', { name: 'Load resources' })).toBeInTheDocument();
    expect(screen.getByText('Resources are loaded only when you request them.')).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('enumerates resources only after an explicit load action', async () => {
    mockedInvoke.mockResolvedValueOnce({ status: 'unverified', windows });
    renderDesktop();
    expandSection();

    fireEvent.click(screen.getByRole('button', { name: 'Load resources' }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', {
        instanceId: 'computer-a',
        uri: null,
      });
    });
    expect(await screen.findByText('Main Window')).toBeInTheDocument();
    expect(screen.getAllByText('Desktop MCP')).toHaveLength(2);
    expect(screen.getByText('text/plain')).toBeInTheDocument();
    expect(screen.getByText('Some Desktop Resources may be missing')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeInTheDocument();
  });

  it('shows an actionable Runtime state without invoking the backend', () => {
    const onStartRuntime = vi.fn();
    renderDesktop({
      runtime: runtimeSnapshot({ lifecycle: 'stopped', active_mcp_servers: 0 }),
      onStartRuntime,
    });
    expandSection();

    expect(screen.getByText('Runtime is not ready')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Start' }));
    expect(onStartRuntime).toHaveBeenCalledTimes(1);
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('shows an actionable no-active-MCP state', () => {
    const onOpenMcp = vi.fn();
    renderDesktop({
      runtime: runtimeSnapshot({
        lifecycle: 'started',
        mcp_servers: 1,
        active_mcp_servers: 0,
      }),
      onOpenMcp,
    });
    expandSection();

    expect(screen.getByText('No MCP server is active')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Manage MCP Servers' }));
    expect(onOpenMcp).toHaveBeenCalledTimes(1);
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('reads one resource only when its row is expanded', async () => {
    setDesktopState('computer-a', { windows, loaded: true });
    mockedInvoke.mockResolvedValueOnce({
      bundleId: 'desktop-bundle',
      uri: 'window://main',
      title: 'Main Window',
      server: 'Desktop MCP',
      contents: [
        { type: 'text', uri: 'window://main', mime_type: 'text/plain', text: 'Hello World' },
      ],
    });
    renderDesktop();
    expandSection();

    expect(mockedInvoke).not.toHaveBeenCalled();
    fireEvent.click(firstRowExpandButton());

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledTimes(1);
      expect(mockedInvoke).toHaveBeenCalledWith('get_window_detail', {
        instanceId: 'computer-a',
        bundleId: 'desktop-bundle',
        uri: 'window://main',
      });
    });
    expect(await screen.findByText('Hello World')).toBeInTheDocument();
  });

  it('does not claim an unverified empty enumeration is a real empty resource set', async () => {
    mockedInvoke.mockResolvedValueOnce({ status: 'unverified', windows: [] });
    renderDesktop();
    expandSection();

    fireEvent.click(screen.getByRole('button', { name: 'Load resources' }));

    expect(
      await screen.findByText('Desktop Resources could not be determined'),
    ).toBeInTheDocument();
    expect(
      screen.queryByText('No desktop resources are currently exposed'),
    ).not.toBeInTheDocument();
  });

  it('does not read a cached detail again after collapse and re-expand', async () => {
    const detail = {
      bundleId: 'desktop-bundle',
      uri: 'window://main',
      title: 'Main Window',
      server: 'Desktop MCP',
      contents: [],
    };
    const key = desktopWindowKey(detail);
    setDesktopState('computer-a', {
      windows,
      loaded: true,
      windowDetails: { [key]: detail },
    });
    renderDesktop();
    expandSection();

    fireEvent.click(firstRowExpandButton());
    fireEvent.click(firstRowExpandButton());
    fireEvent.click(firstRowExpandButton());

    expect(mockedInvoke).not.toHaveBeenCalled();
    expect(screen.getByText('No content available')).toBeInTheDocument();
  });

  it('renders detail failures inline with an explicit retry', async () => {
    const key = desktopWindowKey(windows[0]);
    setDesktopState('computer-a', {
      windows,
      loaded: true,
      detailErrors: { [key]: 'Read failed' },
    });
    mockedInvoke.mockResolvedValueOnce({
      bundleId: 'desktop-bundle',
      uri: 'window://main',
      server: 'Desktop MCP',
      contents: [],
    });
    renderDesktop();
    expandSection();
    fireEvent.click(firstRowExpandButton());

    expect(screen.getByText('Read failed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('get_window_detail', {
        instanceId: 'computer-a',
        bundleId: 'desktop-bundle',
        uri: 'window://main',
      });
    });
  });

  it('renders safe raster previews but does not embed unsupported binary MIME types', () => {
    const pngKey = desktopWindowKey(windows[1]);
    setDesktopState('computer-a', {
      windows: [windows[1]],
      loaded: true,
      windowDetails: {
        [pngKey]: {
          bundleId: 'desktop-bundle',
          uri: 'window://secondary',
          server: 'Desktop MCP',
          contents: [{
            type: 'blob',
            uri: 'window://secondary',
            mime_type: 'image/png',
            blob: 'iVBORw0KGgo=',
          }],
        },
      },
    });
    const { unmount } = renderDesktop();
    expandSection();
    fireEvent.click(firstRowExpandButton());

    expect(screen.getByAltText('Desktop resource preview')).toHaveAttribute(
      'src',
      expect.stringContaining('data:image/png;base64,'),
    );

    unmount();
    useDesktopStore.getState().reset();
    const svgWindow = { ...windows[0], mime_type: 'image/svg+xml' };
    const svgKey = desktopWindowKey(svgWindow);
    setDesktopState('computer-a', {
      windows: [svgWindow],
      loaded: true,
      windowDetails: {
        [svgKey]: {
          bundleId: svgWindow.bundleId,
          uri: svgWindow.uri,
          server: svgWindow.server,
          contents: [{
            type: 'blob',
            uri: svgWindow.uri,
            mime_type: 'image/svg+xml',
            blob: 'PHN2Zz48L3N2Zz4=',
          }],
        },
      },
    });
    renderDesktop();
    expandSection();
    fireEvent.click(firstRowExpandButton());

    expect(screen.queryByAltText('Desktop resource preview')).not.toBeInTheDocument();
    expect(screen.getByText('Binary data is not rendered (11 bytes)')).toBeInTheDocument();
  });

  it('shows only the selected Computer cache', () => {
    setDesktopState('computer-a', {
      windows: [windows[0]],
      loaded: true,
    });
    setDesktopState('computer-b', {
      runtimeKey: activeRuntimeKey,
      windows: [{
        bundleId: 'other',
        uri: 'window://other',
        title: 'Other Computer Window',
        server: 'Other MCP',
      }],
      loaded: true,
    });
    const { rerender } = renderDesktop();
    expandSection();
    expect(screen.getByText('Main Window')).toBeInTheDocument();

    rerender(
      <DesktopResources
        instanceId="computer-b"
        runtime={activeRuntime}
      />,
    );

    expect(screen.getByText('Other Computer Window')).toBeInTheDocument();
    expect(screen.queryByText('Main Window')).not.toBeInTheDocument();
  });

  it('hides cached resources immediately when the Runtime generation changes', () => {
    setDesktopState('computer-a', {
      windows: [windows[0]],
      loaded: true,
    });
    const { rerender } = renderDesktop();
    expandSection();
    expect(screen.getByText('Main Window')).toBeInTheDocument();

    rerender(
      <DesktopResources
        instanceId="computer-a"
        runtime={runtimeSnapshot({
          generation: 2,
          mcp_servers: 1,
          active_mcp_servers: 1,
        })}
      />,
    );

    expect(screen.queryByText('Main Window')).not.toBeInTheDocument();
    expect(screen.getByText('Resources are loaded only when you request them.')).toBeInTheDocument();
  });
});
