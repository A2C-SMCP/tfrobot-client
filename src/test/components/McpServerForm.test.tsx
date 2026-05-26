import { render, screen } from '../helpers/render';
import { McpServerForm, parseToolMetaJson } from '@/components/McpConfig/McpServerForm';

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

describe('McpServerForm input attributes (issue #26)', () => {
  // macOS WKWebView auto-capitalizes / auto-corrects technical input
  // (e.g. "npx" → "Npx"), which then fails to spawn. Text inputs must opt out.
  it('disables auto-capitalization/correction/autofill on the command field', () => {
    render(<McpServerForm onSubmit={async () => {}} onCancel={() => {}} />);

    const command = screen.getByPlaceholderText('npx, python, node...');
    expect(command).toHaveAttribute('autocapitalize', 'off');
    expect(command).toHaveAttribute('autocorrect', 'off');
    expect(command).toHaveAttribute('spellcheck', 'false');
    expect(command).toHaveAttribute('autocomplete', 'off');
  });
});
