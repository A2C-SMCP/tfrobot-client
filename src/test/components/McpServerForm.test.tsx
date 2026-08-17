import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi } from 'vitest';
import {
  McpServerForm,
  normalizeToolMeta,
  normalizeToolMetaMap,
  parseToolMetaJson,
} from '@/components/McpConfig/McpServerForm';
import {
  applyConfigEntryEdit,
  buildInputDefinitionChanges,
  draftInputDefinitions,
  parseConfigValue,
  projectConfigEntries,
  serializeConfigEntries,
  serializeConfigValue,
} from '@/components/McpConfig/configValue';
import {
  hasConflictingHttpAuthorization,
  preserveHttpAuthenticationOptions,
} from '@/components/McpConfig/httpAuthentication';
import type { HttpServerConfig } from '@/stores/mcpStore';

const { fetchInputs, inputState } = vi.hoisted(() => ({
  fetchInputs: vi.fn().mockResolvedValue(undefined),
  inputState: {
    loading: false,
    error: null as string | null,
    activeInstanceId: 'computer-a' as string | null,
  },
}));

vi.mock('@/stores/inputStore', () => ({
  useInputStore: (selector: (state: unknown) => unknown) => selector({
    inputs: [
      { type: 'PromptString', id: 'OPENAI_KEY', label: 'OpenAI key', password: true },
      { type: 'PickString', id: 'REGION', options: [
        { label: 'US East', value: 'us-east' },
        { label: 'Europe', value: 'eu' },
      ] },
      { type: 'Command', id: 'SESSION_TOKEN', command: 'token-helper' },
    ],
    fetchInputs,
    ...inputState,
  }),
}));

beforeEach(() => {
  inputState.loading = false;
  inputState.error = null;
  inputState.activeInstanceId = 'computer-a';
  fetchInputs.mockClear();
});

describe('parseToolMetaJson', () => {
  it('returns empty object for undefined/empty input', () => {
    expect(parseToolMetaJson(undefined)).toEqual({ ok: true, value: {} });
    expect(parseToolMetaJson('')).toEqual({ ok: true, value: {} });
  });

  it('rejects invalid JSON', () => {
    expect(parseToolMetaJson('{not valid}')).toEqual({ ok: false, error: 'invalid_json' });
    expect(parseToolMetaJson('just a string')).toEqual({ ok: false, error: 'invalid_json' });
  });

  it('rejects top-level non-object (array)', () => {
    expect(parseToolMetaJson('[1,2,3]')).toEqual({ ok: false, error: 'invalid_format' });
  });

  it('rejects top-level null', () => {
    expect(parseToolMetaJson('null')).toEqual({ ok: false, error: 'invalid_format' });
  });

  it('rejects primitive values — the original bug: {"tool_name": true}', () => {
    expect(parseToolMetaJson('{"tool_name": true}')).toEqual({ ok: false, error: 'invalid_format' });
    expect(parseToolMetaJson('{"tool_name": "string"}')).toEqual({ ok: false, error: 'invalid_format' });
    expect(parseToolMetaJson('{"tool_name": 42}')).toEqual({ ok: false, error: 'invalid_format' });
  });

  it('rejects null values', () => {
    expect(parseToolMetaJson('{"tool_name": null}')).toEqual({ ok: false, error: 'invalid_format' });
  });

  it('rejects array values', () => {
    expect(parseToolMetaJson('{"tool_name": [1,2,3]}')).toEqual({ ok: false, error: 'invalid_format' });
  });

  it('accepts valid tool_meta with object values', () => {
    const result = parseToolMetaJson('{"tool_name": {"auto_apply": true}}');
    expect(result).toEqual({ ok: true, value: { tool_name: { auto_apply: true } } });
  });

  it('accepts valid tool_meta with multiple tools', () => {
    const input = JSON.stringify({
      tool_a: { auto_apply: true, alias: 'a' },
      tool_b: { tags: ['tag1'], ret_object_mapper: { x: 'y' } },
    });
    const result = parseToolMetaJson(input);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.tool_a).toEqual({ auto_apply: true, alias: 'a' });
      expect(result.value.tool_b).toEqual({ tags: ['tag1'], ret_object_mapper: { x: 'y' } });
    }
  });

  it('accepts empty object', () => {
    expect(parseToolMetaJson('{}')).toEqual({ ok: true, value: {} });
  });

  it('rejects mixed valid/invalid — fails on first invalid value', () => {
    const input = '{"good": {"auto_apply": true}, "bad": true}';
    expect(parseToolMetaJson(input)).toEqual({ ok: false, error: 'invalid_format' });
  });
});

describe('normalizeToolMeta', () => {
  it('removes blank aliases so default metadata does not collapse all tool names', () => {
    expect(normalizeToolMeta({ alias: '' })).toBeNull();
    expect(normalizeToolMeta({ alias: '   ', auto_apply: true })).toEqual({ auto_apply: true });
  });

  it('drops blank aliases from parsed per-tool metadata', () => {
    expect(normalizeToolMetaMap({
      echo: { alias: '', tags: ['debug', ''] },
      ping: { alias: 'pong' },
    })).toEqual({
      echo: { tags: ['debug'] },
      ping: { alias: 'pong' },
    });
  });
});

describe('MCP config value sources', () => {
  it('round-trips constants and canonical Input references without materializing values', () => {
    expect(parseConfigValue('debug')).toEqual({ type: 'Constant', value: 'debug' });
    expect(parseConfigValue('${input:REGION}')).toEqual({ type: 'Input', inputId: 'REGION' });
    expect(serializeConfigValue({ type: 'Constant', value: 'debug' })).toBe('debug');
    expect(serializeConfigValue({ type: 'Input', inputId: 'REGION' }))
      .toBe('${input:REGION}');
  });

  it('treats composite strings as constants so imported values are never truncated', () => {
    const composite = 'prefix-${input:REGION}';
    expect(parseConfigValue(composite)).toEqual({ type: 'Constant', value: composite });
    expect(serializeConfigValue(parseConfigValue(composite))).toBe(composite);
  });

  it('projects persisted constants and Input references into the four-type first-level list', () => {
    render(
      <McpServerForm
        instanceId="computer-a"
        initialValues={{
          type: 'Stdio',
          name: 'mixed-values',
          disabled: false,
          forbidden_tools: [],
          tool_meta: {},
          server_parameters: {
            command: 'echo',
            args: [],
            env: { LOG_LEVEL: 'debug', REGION: '${input:REGION}' },
          },
        }}
        onSubmit={async () => {}}
        onCancel={() => {}}
      />,
    );

    expect(screen.getByText('LOG_LEVEL')).toBeInTheDocument();
    expect(screen.getByText('debug')).toBeInTheDocument();
    expect(screen.getByText('Constant')).toBeInTheDocument();
    expect(screen.getByText('PickString')).toBeInTheDocument();
    expect(screen.getByText('REGION · 2')).toBeInTheDocument();
  });

  it('deduplicates shared definitions and reports atomic definition changes', () => {
    const existing = [{
      type: 'PickString' as const,
      id: 'REGION',
      options: [{ label: 'US East', value: 'us-east' }],
    }];
    const initial = projectConfigEntries({ PRIMARY: '${input:REGION}' }, existing);
    const result = serializeConfigEntries([
      ...initial,
      { key: 'SECONDARY', value: { type: 'Input', inputId: 'REGION', definition: existing[0] } },
    ]);

    expect(result).toEqual({
      ok: true,
      values: { PRIMARY: '${input:REGION}', SECONDARY: '${input:REGION}' },
      definitions: existing,
    });
    if (result.ok) {
      expect(buildInputDefinitionChanges(initial, result.definitions, existing)).toEqual({
        upsert: [],
        removeIfUnused: [],
      });
    }
  });

  it('treats an omitted PromptString password flag as false', () => {
    const existing = [{
      type: 'PromptString' as const,
      id: 'REGION',
      password: false,
    }];
    const initial = projectConfigEntries({ REGION: '${input:REGION}' }, existing);
    const next = [{ type: 'PromptString' as const, id: 'REGION' }];

    expect(buildInputDefinitionChanges(initial, next, existing)).toEqual({
      upsert: [],
      removeIfUnused: [],
    });
  });

  it('updates every draft reference when a shared Input definition is edited', () => {
    const original = {
      type: 'PickString' as const,
      id: 'REGION',
      options: [{ label: 'US East', value: 'us-east' }],
    };
    const updated = {
      ...original,
      options: [...original.options, { label: 'Europe', value: 'eu' }],
    };
    const initial = projectConfigEntries({
      PRIMARY: '${input:REGION}',
      SECONDARY: '${input:REGION}',
    }, [original]);

    const next = applyConfigEntryEdit(initial, 0, {
      key: 'PRIMARY',
      value: { type: 'Input', inputId: 'REGION', definition: updated },
    });
    const serialized = serializeConfigEntries(next);

    expect(next[1].value).toEqual({ type: 'Input', inputId: 'REGION', definition: updated });
    expect(serialized).toEqual({
      ok: true,
      values: { PRIMARY: '${input:REGION}', SECONDARY: '${input:REGION}' },
      definitions: [updated],
    });
  });

  it('makes a definition created in the current draft available to later entries', () => {
    const custom = {
      type: 'PromptString' as const,
      id: 'custom',
      description: 'Custom value',
      password: false,
    };
    const entries = applyConfigEntryEdit([], undefined, {
      key: 'FIRST',
      value: { type: 'Input', inputId: 'custom', definition: custom },
    });

    expect(draftInputDefinitions(entries, [])).toEqual([custom]);
  });
});

describe('HTTP OAuth configuration', () => {
  it('discovers OAuth at runtime without exposing or serializing manual configuration', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const initial: HttpServerConfig = {
      type: 'Http',
      name: 'auto-discovery',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      server_parameters: { url: 'https://mcp.example/api', headers: {} },
    };

    render(
      <McpServerForm
        instanceId="computer-a"
        initialValues={initial}
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );

    expect(screen.queryByRole('switch', { name: 'Enable OAuth' })).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox', { name: 'OAuth Resource' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty('oauth');
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty('authPolicy');
  });

  it('leaves authentication fields absent for a new HTTP server', () => {
    expect(preserveHttpAuthenticationOptions()).toEqual({});
  });

  it('preserves imported advanced OAuth configuration verbatim', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const initial: HttpServerConfig = {
      type: 'Http',
      name: 'protected',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      authPolicy: 'oauth',
      oauth: {
        resource: 'https://resource.example/mcp',
        scopes: ['tools.read'],
        clientName: 'Imported',
        mode: {
          type: 'authorizationCode',
          registration: 'preregistered',
          clientId: 'desktop-client',
          clientSecretInput: 'oauth-secret',
        },
      },
      server_parameters: { url: 'https://transport.example/mcp', headers: {} },
    };

    expect(preserveHttpAuthenticationOptions(initial)).toEqual({
      oauth: initial.oauth,
      authPolicy: 'oauth',
    });

    render(
      <McpServerForm
        instanceId="computer-a"
        initialValues={initial}
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      oauth: initial.oauth,
      authPolicy: 'oauth',
    });
  });

  it('preserves explicit OAuth opt-out and legacy proactive configuration', () => {
    const disabled: HttpServerConfig = {
      type: 'Http',
      name: 'public',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      authPolicy: 'disabled',
      server_parameters: { url: 'https://public.example/mcp', headers: {} },
    };
    const legacy: HttpServerConfig = {
      ...disabled,
      name: 'legacy-proactive',
      authPolicy: undefined,
      oauth: {
        scopes: [],
        mode: { type: 'authorizationCode', registration: 'dynamic' },
      },
    };

    expect(preserveHttpAuthenticationOptions(disabled)).toEqual({ authPolicy: 'disabled' });
    expect(preserveHttpAuthenticationOptions(legacy)).toEqual({ oauth: legacy.oauth });
  });

  it('rejects static Authorization headers combined with proactive OAuth', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const initial: HttpServerConfig = {
      type: 'Http',
      name: 'legacy-proactive',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      oauth: {
        scopes: [],
        mode: { type: 'authorizationCode', registration: 'dynamic' },
      },
      server_parameters: {
        url: 'https://protected.example/mcp',
        headers: { authorization: 'Bearer legacy-token' },
      },
    };

    render(
      <McpServerForm
        instanceId="computer-a"
        initialValues={initial}
        onSubmit={onSubmit}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await screen.findByText(
      'Remove the static Authorization header: this server has an explicit OAuth configuration.',
    )).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  }, 15_000);

  it('matches SDK authentication compatibility rules', () => {
    const oauth = {
      scopes: [],
      mode: { type: 'authorizationCode' as const, registration: 'dynamic' as const },
    };

    expect(hasConflictingHttpAuthorization({ oauth }, { authorization: 'Bearer token' }))
      .toBe(true);
    expect(hasConflictingHttpAuthorization(
      { oauth, authPolicy: 'oauth' },
      { Authorization: 'Bearer token' },
    )).toBe(true);

    // A new server and explicit auto/disabled policies may intentionally use static credentials.
    expect(hasConflictingHttpAuthorization({}, { Authorization: 'Bearer token' })).toBe(false);
    expect(hasConflictingHttpAuthorization(
      { oauth, authPolicy: 'auto' },
      { Authorization: 'Bearer token' },
    )).toBe(false);
    expect(hasConflictingHttpAuthorization(
      { authPolicy: 'disabled' },
      { Authorization: 'Bearer token' },
    )).toBe(false);
  }, 10_000);
});

describe('McpServerForm technical fields and config value sources', () => {
  it('fails closed and offers retry when Input definitions cannot be loaded', () => {
    inputState.error = 'load failed';
    render(<McpServerForm instanceId="computer-a" onSubmit={async () => {}} onCancel={() => {}} />);

    expect(screen.getByText(/Input definitions could not be loaded/)).toBeInTheDocument();
    expect(screen.queryByRole('textbox', { name: 'Server Name' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(fetchInputs).toHaveBeenCalledWith('computer-a');
  });

  // macOS WKWebView auto-capitalizes / auto-corrects technical input
  // (e.g. "npx" → "Npx"), which then fails to spawn. Text inputs must opt out.
  it('disables auto-capitalization/correction/autofill on the command field', () => {
    render(<McpServerForm instanceId="computer-a" onSubmit={async () => {}} onCancel={() => {}} />);

    const command = screen.getByPlaceholderText('npx, python, node...');
    expect(command).toHaveAttribute('autocapitalize', 'off');
    expect(command).toHaveAttribute('autocorrect', 'off');
    expect(command).toHaveAttribute('spellcheck', 'false');
    expect(command).toHaveAttribute('autocomplete', 'off');
  });

  it('does not present default tool alias as a server-wide prefix', () => {
    render(<McpServerForm instanceId="computer-a" onSubmit={async () => {}} onCancel={() => {}} />);

    expect(screen.queryByText('Alias Prefix')).not.toBeInTheDocument();
  });

  it('offers Constant, PromptString, PickString, and Command as first-class item types', async () => {
    render(<McpServerForm instanceId="computer-a" onSubmit={async () => {}} onCancel={() => {}} />);

    fireEvent.click(screen.getByRole('button', { name: /Add Variable/ }));
    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Configuration type' }));

    const options = await screen.findAllByRole('option');
    expect(options.map((option) => option.textContent)).toEqual(expect.arrayContaining([
      'Constant',
      'PromptString',
      'PickString',
      'Command',
    ]));
  }, 10_000);

  it('submits a user-entered environment constant as a literal', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<McpServerForm instanceId="computer-a" onSubmit={onSubmit} onCancel={() => {}} />);

    fireEvent.change(screen.getByRole('textbox', { name: 'Server Name' }), {
      target: { value: 'literal-env' },
    });
    fireEvent.change(screen.getByPlaceholderText('npx, python, node...'), {
      target: { value: 'echo' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Add Variable/ }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Environment variable / header name' }), {
      target: { value: 'LOG_LEVEL' },
    });
    fireEvent.change(screen.getByRole('textbox', { name: 'Constant value' }), {
      target: { value: 'debug' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
    await screen.findByText('LOG_LEVEL');

    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].server_parameters.env).toEqual({ LOG_LEVEL: 'debug' });
    expect(onSubmit.mock.calls[0][1]).toEqual({ upsert: [], removeIfUnused: [] });
  }, 20_000);

  it('submitting the secondary form only updates the outer draft', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<McpServerForm instanceId="computer-a" onSubmit={onSubmit} onCancel={() => {}} />);

    fireEvent.click(screen.getByRole('button', { name: /Add Variable/ }));
    const key = screen.getByRole('textbox', { name: 'Environment variable / header name' });
    fireEvent.change(key, { target: { value: 'LOG_LEVEL' } });
    fireEvent.change(screen.getByRole('textbox', { name: 'Constant value' }), {
      target: { value: 'debug' },
    });
    fireEvent.submit(key.closest('form')!);

    expect(await screen.findByText('LOG_LEVEL')).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
  }, 20_000);

  it('creates a PromptString definition and canonical reference in one outer save', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<McpServerForm instanceId="computer-a" onSubmit={onSubmit} onCancel={() => {}} />);

    fireEvent.change(screen.getByRole('textbox', { name: 'Server Name' }), {
      target: { value: 'custom-input' },
    });
    fireEvent.change(screen.getByPlaceholderText('npx, python, node...'), {
      target: { value: 'echo' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Add Variable/ }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Environment variable / header name' }), {
      target: { value: 'API_KEY' },
    });
    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Configuration type' }));
    const promptOptions = await screen.findAllByText('PromptString');
    fireEvent.click(promptOptions[promptOptions.length - 1]);
    fireEvent.change(screen.getByRole('combobox', { name: 'Variable ID' }), {
      target: { value: 'custom' },
    });
    fireEvent.change(screen.getByRole('textbox', { name: 'Description' }), {
      target: { value: 'API key' },
    });
    fireEvent.click(screen.getByRole('switch', { name: 'Password Mode' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));

    expect(await screen.findByText('PromptString')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].server_parameters.env).toEqual({
      API_KEY: '${input:custom}',
    });
    expect(onSubmit.mock.calls[0][1]).toEqual({
      upsert: [{
        type: 'PromptString',
        id: 'custom',
        description: 'API key',
        default: undefined,
        password: true,
      }],
      removeIfUnused: [],
    });
  }, 30_000);

  it('hydrates an existing definition when its ID is typed instead of selected', async () => {
    render(<McpServerForm instanceId="computer-a" onSubmit={async () => {}} onCancel={() => {}} />);

    fireEvent.click(screen.getByRole('button', { name: /Add Variable/ }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Environment variable / header name' }), {
      target: { value: 'REGION' },
    });
    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Configuration type' }));
    const promptOptions = await screen.findAllByText('PromptString');
    fireEvent.click(promptOptions[promptOptions.length - 1]);
    fireEvent.change(screen.getByRole('combobox', { name: 'Variable ID' }), {
      target: { value: 'REGION' },
    });

    expect(await screen.findByText('Input REGION already exists. Changing it updates every reference to this Input.'))
      .toBeInTheDocument();
    expect(screen.getByText('Options')).toBeInTheDocument();
    expect(screen.queryByRole('switch', { name: 'Password Mode' })).not.toBeInTheDocument();
  }, 20_000);
});
