import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi } from 'vitest';
import {
  McpServerForm,
  normalizeToolMeta,
  normalizeToolMetaMap,
  parseToolMetaJson,
} from '@/components/McpConfig/McpServerForm';
import {
  hasConflictingHttpAuthorization,
  preserveHttpAuthenticationOptions,
} from '@/components/McpConfig/httpAuthentication';
import type { HttpServerConfig } from '@/stores/mcpStore';

const { fetchInputs } = vi.hoisted(() => ({
  fetchInputs: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@/stores/inputStore', () => ({
  useInputStore: (selector: (state: unknown) => unknown) => selector({
    inputs: [{ type: 'PromptString', id: 'OPENAI_KEY', label: 'OpenAI key', password: true }],
    fetchInputs,
  }),
}));

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
  });

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
  });
});

describe('McpServerForm input attributes (issue #26)', () => {
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

  it('inserts a canonical Input reference into an environment value', async () => {
    render(<McpServerForm instanceId="computer-a" onSubmit={async () => {}} onCancel={() => {}} />);

    fireEvent.click(screen.getByRole('button', { name: /Add Variable/ }));
    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Use Input' }));
    fireEvent.click(await screen.findByText('OpenAI key (OPENAI_KEY)'));

    expect(screen.getByPlaceholderText('value')).toHaveValue('${input:OPENAI_KEY}');
  });
});
