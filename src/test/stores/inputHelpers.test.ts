import { getInputId, type InputDefinition } from '@/stores/inputStore';

describe('inputStore helpers', () => {
  it('getInputId extracts id from PromptString', () => {
    const input: InputDefinition = { type: 'PromptString', id: 'api_key', label: 'API Key' };
    expect(getInputId(input)).toBe('api_key');
  });

  it('getInputId extracts id from PickString', () => {
    const input: InputDefinition = { type: 'PickString', id: 'env', label: 'Environment', options: [] };
    expect(getInputId(input)).toBe('env');
  });

  it('getInputId extracts id from Command', () => {
    const input: InputDefinition = { type: 'Command', id: 'version', label: 'Version', command: 'node' };
    expect(getInputId(input)).toBe('version');
  });
});
