import { Page } from '@playwright/test';

const mockResponses: Record<string, unknown> = {
  list_computer_instances: [
    {
      id: 'computer-a',
      name: 'Computer A',
      running: true,
      connected: false,
      mcp_server_count: 1,
      robot_binding: null,
      connection: null,
    },
    {
      id: 'computer-b',
      name: 'Second Computer',
      running: true,
      connected: false,
      mcp_server_count: 1,
      robot_binding: {
        employee_id: 1001,
        robot_id: 'robot-b',
        robot_account_id: 2001,
        namespace: 'test',
        robot_name: 'Robot B',
      },
      connection: null,
    },
  ],
  get_mcp_servers_by_instance: {
    'computer-a': [
      {
        name: 'computer-a-stdio-server',
        running: false,
        disabled: false,
        status_message: 'Stopped',
      },
    ],
    'computer-b': [
      {
        name: 'second-stdio-server',
        running: false,
        disabled: false,
        status_message: 'Stopped',
      },
    ],
  },
  get_dashboard_data: {
    computer_total: 2,
    computer_running: 2,
    computer_stopped: 0,
    computer_connected: 0,
    computers: [
      {
        id: 'computer-a',
        name: 'Computer A',
        running: true,
        connected: false,
        mcp_server_count: 1,
        robot_name: null,
        connection_profile: null,
      },
      {
        id: 'computer-b',
        name: 'Second Computer',
        running: true,
        connected: false,
        mcp_server_count: 1,
        robot_name: 'Robot B',
        connection_profile: null,
      },
    ],
    recent_logs: [],
    runtimes: [
      { name: 'Node.js', path: '/usr/local/bin/node', available: true },
      { name: 'Python', path: null, available: false },
      { name: 'uv', path: null, available: false },
      { name: 'pnpm', path: null, available: false },
    ],
  },
  get_computer_overview_data: {
    id: 'computer-a',
    name: 'Computer A',
    running: true,
    connected: false,
    connection_url: null,
    connection_profile: null,
    robot_name: null,
    mcp_total: 1,
    mcp_running: 0,
    mcp_stopped: 1,
    tools_count: 0,
    recent_logs: [],
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
  list_inputs: [],
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
    const getMockResponse = (responses: Record<string, unknown>, cmd: string, args?: unknown) => {
      if (cmd === 'get_mcp_servers') {
        const instanceId = typeof args === 'object' && args !== null && 'instanceId' in args
          ? String((args as { instanceId: unknown }).instanceId)
          : 'computer-a';
        const byInstance = responses.get_mcp_servers_by_instance as Record<string, unknown> | undefined;
        return byInstance?.[instanceId] ?? byInstance?.['computer-a'];
      }
      return responses[cmd];
    };

    (window as any).__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args?: unknown) => {
        ((window as any).__TAURI_INVOKES__ ??= []).push({ cmd, args });
        console.log(`[mock invoke] ${cmd}`, args);
        const response = getMockResponse(data as Record<string, unknown>, cmd, args);
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
