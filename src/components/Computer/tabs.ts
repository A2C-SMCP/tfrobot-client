import type { McpServerManagedBy } from '@/stores/mcpStore';

export const COMPUTER_WORKBENCH_SECTIONS = [
  'top',
  'skills',
  'resources',
  'debug',
  'logs',
] as const;

export type ComputerWorkbenchSection = (typeof COMPUTER_WORKBENCH_SECTIONS)[number];

const LEGACY_WORKBENCH_SECTIONS: Record<string, ComputerWorkbenchSection> = {
  overview: 'top',
  runtime: 'top',
  skills: 'skills',
  resources: 'resources',
  debug: 'debug',
  logs: 'logs',
};

export function toComputerWorkbenchSection(
  value: string | undefined,
): ComputerWorkbenchSection {
  if (!value) return 'top';
  return LEGACY_WORKBENCH_SECTIONS[value]
    ?? (COMPUTER_WORKBENCH_SECTIONS.includes(value as ComputerWorkbenchSection)
      ? value as ComputerWorkbenchSection
      : 'top');
}

export const COMPUTER_SETTINGS_SECTIONS = [
  'general',
  'skills',
  'plugins',
  'mcp',
  'inputs',
  'connection',
  'built-in-tools',
] as const;

export type ComputerSettingsSection = (typeof COMPUTER_SETTINGS_SECTIONS)[number];
export type PluginSettingsTarget = Extract<McpServerManagedBy, { type: 'plugin' }>;

const LEGACY_SETTINGS_TABS: Partial<Record<string, ComputerSettingsSection>> = {
  mcp: 'mcp',
  marketplace: 'plugins',
  inputs: 'inputs',
  connection: 'connection',
  configuration: 'skills',
  'remote-control': 'built-in-tools',
};

export function toComputerSettingsSection(value: string | undefined): ComputerSettingsSection {
  return COMPUTER_SETTINGS_SECTIONS.includes(value as ComputerSettingsSection)
    ? (value as ComputerSettingsSection)
    : 'general';
}

export function legacyComputerSettingsSection(
  value: string | undefined,
): ComputerSettingsSection | null {
  return value ? LEGACY_SETTINGS_TABS[value] ?? null : null;
}

export function computerSettingsNavigationKey(
  section: ComputerSettingsSection,
  targetPlugin?: PluginSettingsTarget | null,
): string {
  if (section !== 'plugins' || !targetPlugin) {
    return `computer-settings:${section}`;
  }
  const target = [
    targetPlugin.marketplace,
    targetPlugin.plugin,
    targetPlugin.pluginId ?? '',
  ].map(encodeURIComponent);
  return `computer-settings:${section}:${target.join(':')}`;
}

export function parsePluginSettingsTarget(parts: string[]): PluginSettingsTarget | null {
  if (parts.length < 2 || !parts[0] || !parts[1]) return null;
  return {
    type: 'plugin',
    marketplace: decodeURIComponent(parts[0]),
    plugin: decodeURIComponent(parts[1]),
    pluginId: parts[2] ? decodeURIComponent(parts[2]) : null,
  };
}
