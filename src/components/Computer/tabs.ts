import type { McpServerManagedBy } from '@/stores/mcpStore';

export const COMPUTER_DETAIL_TABS = ['overview', 'skills', 'resources', 'debug', 'logs', 'runtime'] as const;

export type ComputerDetailTab = (typeof COMPUTER_DETAIL_TABS)[number];

export function toComputerDetailTab(value: string | undefined): ComputerDetailTab {
  return COMPUTER_DETAIL_TABS.includes(value as ComputerDetailTab) ? (value as ComputerDetailTab) : 'overview';
}

export const COMPUTER_SETTINGS_SECTIONS = [
  'general',
  'skills',
  'plugins',
  'mcp',
  'inputs',
  'connection',
] as const;

export type ComputerSettingsSection = (typeof COMPUTER_SETTINGS_SECTIONS)[number];
export type PluginSettingsTarget = Extract<McpServerManagedBy, { type: 'plugin' }>;

const LEGACY_SETTINGS_TABS: Partial<Record<string, ComputerSettingsSection>> = {
  mcp: 'mcp',
  marketplace: 'plugins',
  inputs: 'inputs',
  connection: 'connection',
  configuration: 'skills',
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
