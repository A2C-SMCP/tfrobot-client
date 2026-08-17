export type ConfigValueFormValue =
  | { source: 'constant'; value: string }
  | { source: 'input'; value: string };

const INPUT_REFERENCE = /^\$\{input:([^}]+)\}$/;

export function parseConfigValue(value: string): ConfigValueFormValue {
  const match = INPUT_REFERENCE.exec(value);
  return match
    ? { source: 'input', value: match[1] }
    : { source: 'constant', value };
}

export function serializeConfigValue(value: ConfigValueFormValue | undefined): string {
  if (!value) return '';
  return value.source === 'input' ? `\${input:${value.value}}` : value.value;
}
