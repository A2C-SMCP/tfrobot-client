import { Page } from '@playwright/test';

const mockResponses: Record<string, unknown> = {
  get_mcp_servers: [
    {
      name: 'test-stdio-server',
      type: 'stdio',
      status: 'stopped',
      command: 'node',
      args: ['server.js'],
    },
  ],
  get_dashboard_data: {
    connected: false,
    connection_url: null,
    connection_profile: null,
    mcp_total: 1,
    mcp_running: 0,
    mcp_stopped: 1,
    tools_count: 0,
    recent_logs: [],
    runtimes: [
      { name: 'Node.js', path: '/usr/local/bin/node', available: true },
      { name: 'Python', path: null, available: false },
      { name: 'uv', path: null, available: false },
      { name: 'pnpm', path: null, available: false },
    ],
  },
  list_profiles: [],
  get_connection_status: { connected: false },
  get_settings: {
    theme: 'system',
    language: 'en',
    log_retention_days: 30,
    custom_runtime_paths: {},
  },
  get_logs: [],
  list_input_definitions: [],
  list_input_values: {},
  get_available_tools: [],
  detect_runtimes: { node: '/usr/local/bin/node' },
  get_app_info: { version: '0.1.0', smcp_computer_version: '0.1.8' },
  get_mcp_server_config: null,
  save_settings: null,
  add_mcp_server: null,
  remove_mcp_server: null,
  update_mcp_server: null,
  start_mcp_server: null,
  stop_mcp_server: null,
  start_all_servers: null,
  stop_all_servers: null,
};

export async function setupInvokeMock(page: Page, overrides?: Record<string, unknown>) {
  const responses = { ...mockResponses, ...overrides };

  await page.addInitScript((data) => {
    (window as any).__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args?: unknown) => {
        console.log(`[mock invoke] ${cmd}`, args);
        const response = (data as Record<string, unknown>)[cmd];
        if (response !== undefined) {
          return Promise.resolve(JSON.parse(JSON.stringify(response)));
        }
        console.warn(`[mock invoke] No mock for command: ${cmd}`);
        return Promise.resolve(null);
      },
      transformCallback: () => 0,
    };

    // Mock event listener
    (window as any).__TAURI_INTERNALS__.metadata = {
      currentWebview: { label: 'main' },
      currentWindow: { label: 'main' },
    };
  }, responses);
}
