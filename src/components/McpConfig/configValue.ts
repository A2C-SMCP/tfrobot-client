import type { InputDefinition, InputDefinitionChanges } from '@/stores/inputStore';

export type ConfigValueFormValue =
  | { type: 'Constant'; value: string }
  | { type: 'Input'; inputId: string; definition?: InputDefinition };

export interface ConfigEntryFormValue {
  key: string;
  value: ConfigValueFormValue;
}

const INPUT_REFERENCE = /^\$\{input:([^}]+)\}$/;

export function parseConfigValue(
  value: string,
  definitions: InputDefinition[] = [],
): ConfigValueFormValue {
  const match = INPUT_REFERENCE.exec(value);
  if (!match) return { type: 'Constant', value };
  const inputId = match[1];
  return {
    type: 'Input',
    inputId,
    definition: definitions.find((definition) => definition.id === inputId),
  };
}

export function serializeConfigValue(value: ConfigValueFormValue | undefined): string {
  if (!value) return '';
  return value.type === 'Input' ? `\${input:${value.inputId}}` : value.value;
}

export function projectConfigEntries(
  values: Record<string, string>,
  definitions: InputDefinition[],
): ConfigEntryFormValue[] {
  return Object.entries(values).map(([key, value]) => ({
    key,
    value: parseConfigValue(value, definitions),
  }));
}

export function draftInputDefinitions(
  entries: ConfigEntryFormValue[],
  availableDefinitions: InputDefinition[],
): InputDefinition[] {
  const definitions = new Map(
    availableDefinitions.map((definition) => [definition.id, definition]),
  );
  for (const entry of entries) {
    if (entry.value.type === 'Input' && entry.value.definition) {
      definitions.set(entry.value.inputId, entry.value.definition);
    }
  }
  return [...definitions.values()];
}

export function applyConfigEntryEdit(
  entries: ConfigEntryFormValue[],
  index: number | undefined,
  entry: ConfigEntryFormValue,
): ConfigEntryFormValue[] {
  const next = index === undefined
    ? [...entries, entry]
    : entries.map((current, currentIndex) => (currentIndex === index ? entry : current));

  if (entry.value.type !== 'Input' || !entry.value.definition) return next;
  const { inputId, definition } = entry.value;
  return next.map((current) => (
    current.value.type === 'Input' && current.value.inputId === inputId
      ? { ...current, value: { ...current.value, definition } }
      : current
  ));
}

function sameDefinition(left: InputDefinition, right: InputDefinition): boolean {
  const effectiveDescription = (definition: InputDefinition) => (
    definition.label?.trim()
    || ('description' in definition ? definition.description?.trim() : undefined)
    || definition.id
  );
  if (
    left.type !== right.type
    || left.id !== right.id
    || effectiveDescription(left) !== effectiveDescription(right)
  ) return false;
  if (left.type === 'PromptString' && right.type === 'PromptString') {
    return left.default === right.default
      && (left.password ?? false) === (right.password ?? false);
  }
  if (left.type === 'PickString' && right.type === 'PickString') {
    return left.default === right.default
      && left.options.length === right.options.length
      && left.options.every((option, index) => (
        option.label === right.options[index].label && option.value === right.options[index].value
      ));
  }
  if (left.type === 'Command' && right.type === 'Command') {
    const leftArgs = left.args ?? [];
    const rightArgs = right.args ?? [];
    return left.command === right.command
      && leftArgs.length === rightArgs.length
      && leftArgs.every((arg, index) => arg === rightArgs[index]);
  }
  return false;
}

export type SerializeConfigEntriesResult =
  | {
    ok: true;
    values: Record<string, string>;
    definitions: InputDefinition[];
  }
  | { ok: false; error: 'missing_definition' | 'conflicting_definition'; inputId: string };

export function serializeConfigEntries(
  entries: ConfigEntryFormValue[] | undefined,
): SerializeConfigEntriesResult {
  const values: Record<string, string> = {};
  const definitions = new Map<string, InputDefinition>();

  for (const entry of entries ?? []) {
    if (!entry.key) continue;
    values[entry.key] = serializeConfigValue(entry.value);
    if (entry.value.type !== 'Input') continue;
    if (!entry.value.definition) {
      return { ok: false, error: 'missing_definition', inputId: entry.value.inputId };
    }
    const existing = definitions.get(entry.value.inputId);
    if (existing && !sameDefinition(existing, entry.value.definition)) {
      return { ok: false, error: 'conflicting_definition', inputId: entry.value.inputId };
    }
    definitions.set(entry.value.inputId, entry.value.definition);
  }

  return { ok: true, values, definitions: [...definitions.values()] };
}

export function buildInputDefinitionChanges(
  initialEntries: ConfigEntryFormValue[],
  nextDefinitions: InputDefinition[],
  availableDefinitions: InputDefinition[],
): InputDefinitionChanges {
  const currentById = new Map(availableDefinitions.map((definition) => [definition.id, definition]));
  const nextIds = new Set(nextDefinitions.map((definition) => definition.id));
  const initialIds = new Set(
    initialEntries
      .filter((entry) => entry.value.type === 'Input')
      .map((entry) => (entry.value as Extract<ConfigValueFormValue, { type: 'Input' }>).inputId),
  );

  return {
    upsert: nextDefinitions.filter((definition) => {
      const current = currentById.get(definition.id);
      return !current || !sameDefinition(current, definition);
    }),
    removeIfUnused: [...initialIds].filter((id) => !nextIds.has(id)),
  };
}
