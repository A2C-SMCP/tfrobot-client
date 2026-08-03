import { Page } from '@playwright/test';

const startedRuntime = {
  incarnation: 1,
  generation: 1,
  snapshot_revision: 1,
  lifecycle: 'started',
  user_state: 'running',
  actions: {
    start: { enabled: false, disabled_reason: 'already_running' },
    stop: { enabled: true, disabled_reason: null },
    restart: { enabled: true, disabled_reason: null },
    connect: { enabled: true, disabled_reason: null },
    disconnect: { enabled: false, disabled_reason: 'connection_unavailable' },
    manage_mcp: { enabled: true, disabled_reason: null },
  },
  config_revision: 1,
  capability_revision: 1,
  mcp_servers: 1,
  active_mcp_servers: 0,
  tools: 0,
  skills: 0,
  problems: [],
  last_error: null,
  degraded_reason: null,
};

const shutdownRuntime = {
  ...startedRuntime,
  lifecycle: 'shutdown',
  user_state: 'not_running',
  actions: {
    start: { enabled: true, disabled_reason: null },
    stop: { enabled: false, disabled_reason: 'not_running' },
    restart: { enabled: false, disabled_reason: 'not_running' },
    connect: { enabled: false, disabled_reason: 'not_running' },
    disconnect: { enabled: false, disabled_reason: 'not_running' },
    manage_mcp: { enabled: false, disabled_reason: 'not_running' },
  },
  mcp_servers: 0,
};

const disconnectedAuthority = { present: false, revision: 0, context: null };

const mockResponses: Record<string, unknown> = {
  enable_computer_runtime_events: [
    { instance_id: 'computer-a', snapshot: startedRuntime, connection: disconnectedAuthority },
    { instance_id: 'computer-b', snapshot: startedRuntime, connection: disconnectedAuthority },
  ],
  get_computer_runtime_snapshots: [
    { instance_id: 'computer-a', snapshot: startedRuntime, connection: disconnectedAuthority },
    { instance_id: 'computer-b', snapshot: startedRuntime, connection: disconnectedAuthority },
  ],
  list_computer_instances: [
    {
      id: 'computer-a',
      name: 'Computer A',
      description: 'Primary test computer',
      local_skills_root: null,
      default_skill_home: '/mock/computer_instances/computer-a/skill_home',
      configured_skill_home: '/mock/computer_instances/computer-a/skill_home',
      effective_skill_home: '/mock/computer_instances/computer-a/skill_home',
      running: true,
      runtime: startedRuntime,
      connected: false,
      client_connection_present: false,
      connection_revision: 0,
      connection_context: null,
      mcp_server_count: 1,
      robot_binding: null,
      connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
      connection: null,
    },
    {
      id: 'computer-b',
      name: 'Second Computer',
      description: 'Secondary test computer',
      running: true,
      runtime: startedRuntime,
      connected: false,
      client_connection_present: false,
      connection_revision: 0,
      connection_context: null,
      mcp_server_count: 1,
      robot_binding: {
        employee_id: 1001,
        robot_id: 'robot-b',
        robot_account_id: '2001',
        namespace: 'test',
        robot_name: 'Robot B',
      },
      connection_policy: { target: { type: 'manager_robot', id: '1001' }, auto_connect: false },
      connection: null,
    },
  ],
  get_mcp_servers_by_instance: {
    'computer-a': [
      {
        bundleId: 'computer-a-stdio-server',
        name: 'computer-a-stdio-server',
        running: false,
        disabled: false,
        status_message: 'Stopped',
        managedBy: { type: 'user' },
      },
      {
        bundleId: 'plugin-tools',
        name: 'plugin-tools',
        running: false,
        disabled: false,
        status_message: 'Stopped',
        managedBy: {
          type: 'plugin',
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
        },
      },
    ],
    'computer-b': [
      {
        bundleId: 'second-stdio-server',
        name: 'second-stdio-server',
        running: false,
        disabled: false,
        status_message: 'Stopped',
        managedBy: { type: 'user' },
      },
    ],
  },
  get_computer_config_state_by_instance: {
    'computer-a': {
      snapshot: {
        version: 1,
        revision: 'sha256:computer-a',
        mcp: {
          servers: [
            {
              bundleId: 'computer-a-stdio-server',
              name: 'computer-a-stdio-server',
              origin: 'local',
              writable: true,
              trustedOrigin: false,
              bundled: false,
              config: {
                type: 'Stdio',
                name: 'computer-a-stdio-server',
                disabled: false,
                forbidden_tools: [],
                tool_meta: {},
                server_parameters: { command: 'node', args: [], env: {} },
              },
            },
            {
              bundleId: 'plugin-tools',
              name: 'plugin-tools',
              origin: 'plugin',
              writable: false,
              trustedOrigin: true,
              bundled: true,
              config: {
                type: 'Stdio',
                name: 'plugin-tools',
                disabled: false,
                forbidden_tools: [],
                tool_meta: {},
                server_parameters: { command: 'plugin-tools', args: [], env: {} },
              },
            },
          ],
        },
        provenance: {},
      },
      validation: { valid: true, errors: [] },
    },
    'computer-b': {
      snapshot: {
        version: 1,
        revision: 'sha256:computer-b',
        mcp: {
          servers: [
            {
              bundleId: 'second-stdio-server',
              name: 'second-stdio-server',
              origin: 'local',
              writable: true,
              trustedOrigin: false,
              bundled: false,
              config: {
                type: 'Stdio',
                name: 'second-stdio-server',
                disabled: false,
                forbidden_tools: [],
                tool_meta: {},
                server_parameters: { command: 'node', args: [], env: {} },
              },
            },
          ],
        },
        provenance: {},
      },
      validation: { valid: true, errors: [] },
    },
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
        runtime: startedRuntime,
        connected: false,
        client_connection_present: false,
        connection_revision: 0,
        connection_context: null,
        mcp_server_count: 1,
        robot_name: null,
        connection_profile: null,
      },
      {
        id: 'computer-b',
        name: 'Second Computer',
        running: true,
        runtime: startedRuntime,
        connected: false,
        client_connection_present: false,
        connection_revision: 0,
        connection_context: null,
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
  create_computer_instance: {
    id: 'computer-created',
    name: 'Created Computer',
    description: null,
    running: false,
    runtime: shutdownRuntime,
    connected: false,
    client_connection_present: false,
    connection_revision: 0,
    connection_context: null,
    mcp_server_count: 0,
    robot_binding: null,
    connection_policy: { target: null, auto_connect: false },
    connection: null,
  },
  rename_computer_instance: {
    id: 'computer-a',
    name: 'Renamed Computer',
    description: null,
    running: true,
    runtime: startedRuntime,
    connected: false,
    client_connection_present: false,
    connection_revision: 0,
    connection_context: null,
    mcp_server_count: 1,
    robot_binding: null,
    connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    connection: null,
  },
  duplicate_computer_instance: {
    id: 'computer-copy',
    name: 'Computer A Copy',
    description: 'Primary test computer',
    running: false,
    runtime: shutdownRuntime,
    connected: false,
    client_connection_present: false,
    connection_revision: 0,
    connection_context: null,
    mcp_server_count: 1,
    robot_binding: null,
    connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    connection: null,
  },
  delete_computer_instance: null,
  start_computer_instance: {
    id: 'computer-a',
    name: 'Computer A',
    description: 'Primary test computer',
    running: true,
    runtime: { ...startedRuntime, snapshot_revision: 2 },
    connected: false,
    client_connection_present: false,
    connection_revision: 0,
    connection_context: null,
    mcp_server_count: 1,
    robot_binding: null,
    connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    connection: null,
  },
  stop_computer_instance: {
    id: 'computer-a',
    name: 'Computer A',
    description: 'Primary test computer',
    running: false,
    runtime: { ...shutdownRuntime, snapshot_revision: 2 },
    connected: false,
    client_connection_present: false,
    connection_revision: 0,
    connection_context: null,
    mcp_server_count: 1,
    robot_binding: null,
    connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    connection: null,
  },
  list_manual_smcp_targets: [
    {
      id: 'target-a',
      name: 'Target A',
      url: 'https://smcp.example.com',
      namespace: '/smcp',
      office_id: 'office-a',
      headers: {},
    },
  ],
  update_computer_connection_policy: {
    id: 'computer-a',
    name: 'Computer A',
    description: 'Primary test computer',
    running: true,
    runtime: startedRuntime,
    connected: false,
    client_connection_present: false,
    connection_revision: 0,
    connection_context: null,
    mcp_server_count: 1,
    robot_binding: null,
    connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    connection: null,
  },
  connect_computer_connection_target: null,
  disconnect_computer_connection_target: null,
  get_connection_status: { connected: false },
  get_settings: {
    theme: 'system',
    language: 'en',
    log_retention_days: 30,
    custom_runtime_paths: {},
  },
  get_logs: [],
  list_skills: [
    {
      name: 'keyboard-helper',
      source: 'user',
      path: '/skills/user/keyboard-helper',
      description: 'Keyboard accessible helper',
    },
  ],
  get_skill: {
    name: 'keyboard-helper',
    relPath: 'SKILL.md',
    mimeType: 'text/markdown',
    totalSize: 40,
    sha256: 'keyboard-helper',
    isEntry: true,
    isText: true,
    body: '# Keyboard Helper\n\nOpened from the keyboard.',
  },
  get_marketplace_governance: {
    capabilities: {
      computerLifecycleApiAvailable: true,
      supportedOperations: [
        'add_marketplace',
        'update_marketplace',
        'refresh_marketplace',
        'remove_marketplace',
        'install_plugin',
        'enable_plugin',
        'disable_plugin',
        'uninstall_plugin',
      ],
      requiredSdkApis: [],
      reason: 'available in E2E fixture',
    },
    marketplaces: [
      {
        name: 'acme',
        displayGitUrl: 'https://example.com/acme.git',
        status: 'ready',
        message: null,
      },
    ],
    plugins: [
      {
        marketplace: 'acme',
        plugin: 'audit',
        pluginId: 'plugin-old',
        version: '0.9.0',
        installed: true,
        enabled: true,
        status: 'enabled',
        bundledMcpServers: ['legacy-audit-mcp'],
        bundledSkills: [],
        declared: null,
        message: 'Previous owner',
      },
      {
        marketplace: 'acme',
        plugin: 'audit',
        pluginId: 'plugin-2',
        version: '1.0.0',
        installed: true,
        enabled: true,
        status: 'enabled',
        bundledMcpServers: ['plugin-tools'],
        bundledSkills: [],
        declared: null,
        message: null,
      },
    ],
  },
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
    const callbacks = new Map<number, (...args: unknown[]) => unknown>();
    let nextCallbackId = 1;
    const getMockResponse = (responses: Record<string, unknown>, cmd: string, args?: unknown) => {
      if (cmd === 'get_mcp_servers') {
        const instanceId = typeof args === 'object' && args !== null && 'instanceId' in args
          ? String((args as { instanceId: unknown }).instanceId)
          : 'computer-a';
        const byInstance = responses.get_mcp_servers_by_instance as Record<string, unknown> | undefined;
        return byInstance?.[instanceId] ?? byInstance?.['computer-a'];
      }
      if (cmd === 'get_computer_config_state') {
        const instanceId = typeof args === 'object' && args !== null && 'instanceId' in args
          ? String((args as { instanceId: unknown }).instanceId)
          : 'computer-a';
        const byInstance = responses.get_computer_config_state_by_instance as Record<string, unknown> | undefined;
        return byInstance?.[instanceId] ?? byInstance?.['computer-a'];
      }
      return responses[cmd];
    };

    (window as any).__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args?: unknown) => {
        ((window as any).__TAURI_INVOKES__ ??= []).push({ cmd, args });
        console.log(`[mock invoke] ${cmd}`, args);
        if (cmd === 'plugin:event|listen') {
          return Promise.resolve((args as { handler?: number } | undefined)?.handler ?? null);
        }
        if (cmd === 'plugin:event|unlisten') {
          return Promise.resolve(null);
        }
        const response = getMockResponse(data as Record<string, unknown>, cmd, args);
        if (response !== undefined) {
          return Promise.resolve(JSON.parse(JSON.stringify(response)));
        }
        console.warn(`[mock invoke] No mock for command: ${cmd}`);
        return Promise.resolve(null);
      },
      transformCallback: (callback: (...args: unknown[]) => unknown, once = false) => {
        const id = nextCallbackId++;
        callbacks.set(id, (...args: unknown[]) => {
          if (once) callbacks.delete(id);
          return callback(...args);
        });
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      runCallback: (id: number, ...args: unknown[]) => callbacks.get(id)?.(...args),
      callbacks,
    };

    (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: (_event: string, id: number) => callbacks.delete(id),
    };

    // Mock event listener
    (window as any).__TAURI_INTERNALS__.metadata = {
      currentWebview: { label: 'main' },
      currentWindow: { label: 'main' },
    };
  }, responses);
}
