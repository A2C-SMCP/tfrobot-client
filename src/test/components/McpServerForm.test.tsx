import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { vi } from 'vitest';
import {
  McpServerForm,
  buildHttpOAuthOptions,
  hasStaticAuthorizationHeader,
  normalizeToolMeta,
  normalizeToolMetaMap,
  parseToolMetaJson,
} from '@/components/McpConfig/McpServerForm';
import type { HttpServerConfig, OAuthClientMode } from '@/stores/mcpStore';

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
  it('creates dynamic Authorization Code configuration without client credentials or scopes', () => {
    expect(buildHttpOAuthOptions(true, undefined, 'https://mcp.example/api'))
      .toEqual({
        resource: undefined,
        scopes: [],
        mode: { type: 'authorizationCode', registration: 'dynamic' },
      });
  });

  it('preserves imported preregistered and client metadata configuration verbatim', () => {
    const modes: OAuthClientMode[] = [
      { type: 'authorizationCode', registration: 'preregistered', clientId: 'client', clientSecretInput: 'secret' },
      { type: 'authorizationCode', registration: 'clientMetadataDocument', url: 'https://client.example/metadata.json' },
    ];
    for (const mode of modes) {
      const initial: HttpServerConfig = {
        type: 'Http',
        name: 'protected',
        disabled: false,
        forbidden_tools: [],
        tool_meta: {},
        oauth: { resource: undefined, scopes: ['tools.read'], clientName: 'Imported', mode },
        server_parameters: { url: 'https://mcp.example/api', headers: {} },
      };
      expect(buildHttpOAuthOptions(true, initial.server_parameters.url, initial.server_parameters.url, initial))
        .toBe(initial.oauth);
    }
  });

  it('preserves implicit resource semantics when only the server URL is edited', () => {
    const initial: HttpServerConfig = {
      type: 'Http',
      name: 'protected',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      oauth: {
        resource: undefined,
        scopes: [],
        mode: { type: 'authorizationCode', registration: 'dynamic' },
      },
      server_parameters: { url: 'https://old.example/mcp', headers: {} },
    };

    expect(buildHttpOAuthOptions(
      true,
      'https://old.example/mcp',
      'https://new.example/mcp',
      initial,
    )).toEqual(initial.oauth);
  });

  it('pins an implicit displayed resource only after the resource field is explicitly edited', () => {
    const initial: HttpServerConfig = {
      type: 'Http',
      name: 'protected',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      oauth: {
        resource: undefined,
        scopes: [],
        mode: { type: 'authorizationCode', registration: 'dynamic' },
      },
      server_parameters: { url: 'https://old.example/mcp', headers: {} },
    };

    expect(buildHttpOAuthOptions(
      true,
      'https://old.example/mcp',
      'https://new.example/mcp',
      initial,
      true,
    )).toEqual({
      ...initial.oauth,
      resource: 'https://old.example/mcp',
    });
  });

  it('keeps a new resource implicit until the user explicitly edits it', () => {
    expect(buildHttpOAuthOptions(
      true,
      'https://old.example/mcp',
      'https://new.example/mcp',
      undefined,
      false,
    )).toEqual({
      resource: undefined,
      scopes: [],
      mode: { type: 'authorizationCode', registration: 'dynamic' },
    });

    expect(buildHttpOAuthOptions(
      true,
      'https://resource.example/mcp',
      'https://new.example/mcp',
      undefined,
      true,
    )).toEqual({
      resource: 'https://resource.example/mcp',
      scopes: [],
      mode: { type: 'authorizationCode', registration: 'dynamic' },
    });

    expect(buildHttpOAuthOptions(
      true,
      'https://new.example/mcp',
      'https://new.example/mcp',
      undefined,
      true,
    )).toEqual({
      resource: 'https://new.example/mcp',
      scopes: [],
      mode: { type: 'authorizationCode', registration: 'dynamic' },
    });
  });

  it('restores implicit resource semantics when an explicit Resource is cleared', () => {
    const initial: HttpServerConfig = {
      type: 'Http',
      name: 'protected',
      disabled: false,
      forbidden_tools: [],
      tool_meta: {},
      oauth: {
        resource: 'https://resource.example/mcp',
        scopes: [],
        mode: { type: 'authorizationCode', registration: 'dynamic' },
      },
      server_parameters: { url: 'https://old.example/mcp', headers: {} },
    };

    expect(buildHttpOAuthOptions(
      true,
      undefined,
      'https://new.example/mcp',
      initial,
      true,
    )).toEqual({
      ...initial.oauth,
      resource: undefined,
    });
  });

  it('submits the latest URL as the implicit resource after OAuth is enabled', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<McpServerForm instanceId="computer-a" onSubmit={onSubmit} onCancel={() => {}} />);

    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Server Type' }));
    fireEvent.click(await screen.findByText('HTTP'));
    fireEvent.change(screen.getByRole('textbox', { name: 'Server Name' }), {
      target: { value: 'Protected MCP' },
    });
    const url = screen.getByRole('textbox', { name: 'URL' });
    fireEvent.change(url, { target: { value: 'https://old.example/mcp' } });
    fireEvent.click(screen.getByRole('switch', { name: 'Enable OAuth' }));
    expect(screen.getByRole('textbox', { name: 'OAuth Resource' }))
      .toHaveAttribute('placeholder', 'https://old.example/mcp');

    fireEvent.change(url, { target: { value: 'https://new.example/mcp' } });
    expect(screen.getByRole('textbox', { name: 'OAuth Resource' }))
      .toHaveAttribute('placeholder', 'https://new.example/mcp');
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      type: 'Http',
      oauth: { resource: undefined },
      server_parameters: { url: 'https://new.example/mcp' },
    });
  }, 10_000);

  it('keeps an explicitly entered Resource fixed when it equals the current URL', async () => {
    const create = vi.fn().mockResolvedValue(undefined);
    const created = render(
      <McpServerForm instanceId="computer-a" onSubmit={create} onCancel={() => {}} />,
    );

    fireEvent.mouseDown(screen.getByRole('combobox', { name: 'Server Type' }));
    fireEvent.click(await screen.findByText('HTTP'));
    fireEvent.change(screen.getByRole('textbox', { name: 'Server Name' }), {
      target: { value: 'Pinned Resource MCP' },
    });
    const url = screen.getByRole('textbox', { name: 'URL' });
    fireEvent.change(url, { target: { value: 'https://resource.example/mcp' } });
    fireEvent.click(screen.getByRole('switch', { name: 'Enable OAuth' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'OAuth Resource' }), {
      target: { value: 'https://resource.example/mcp' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));

    await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
    const createdConfig = create.mock.calls[0][0] as HttpServerConfig;
    expect(createdConfig.oauth?.resource).toBe('https://resource.example/mcp');

    created.unmount();
    const update = vi.fn().mockResolvedValue(undefined);
    render(
      <McpServerForm
        instanceId="computer-a"
        initialValues={createdConfig}
        onSubmit={update}
        onCancel={() => {}}
      />,
    );
    fireEvent.change(screen.getByRole('textbox', { name: 'URL' }), {
      target: { value: 'https://new.example/mcp' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(update).toHaveBeenCalledTimes(1));
    expect(update.mock.calls[0][0]).toMatchObject({
      oauth: { resource: 'https://resource.example/mcp' },
      server_parameters: { url: 'https://new.example/mcp' },
    });
  }, 15_000);

  it('detects Authorization headers case-insensitively', () => {
    expect(hasStaticAuthorizationHeader({ authorization: 'Bearer static' })).toBe(true);
    expect(hasStaticAuthorizationHeader({ 'X-Test': 'value' })).toBe(false);
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
