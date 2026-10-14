/**
 * Settings tabs are routable so a notice can lead straight to the section that explains it:
 * `settings:<tab>` or `settings:<tab>:<anchor>`. Keep this list in sync with the Tabs rendered by
 * `Settings`.
 */
export const SETTINGS_TABS = ['appearance', 'runtime', 'data', 'permissions', 'about'] as const;

export type SettingsTab = (typeof SETTINGS_TABS)[number];

export function toSettingsTab(value: string | undefined): SettingsTab {
  return SETTINGS_TABS.includes(value as SettingsTab) ? (value as SettingsTab) : 'appearance';
}

export function settingsNavigationKey(tab: SettingsTab, anchor?: string | null): string {
  return anchor ? `settings:${tab}:${anchor}` : `settings:${tab}`;
}
