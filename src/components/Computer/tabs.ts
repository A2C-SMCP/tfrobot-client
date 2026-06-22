export const COMPUTER_DETAIL_TABS = ['mcp', 'inputs', 'connection', 'resources', 'debug', 'runtime'] as const;

export type ComputerDetailTab = (typeof COMPUTER_DETAIL_TABS)[number];

export function toComputerDetailTab(value: string | undefined): ComputerDetailTab {
  return COMPUTER_DETAIL_TABS.includes(value as ComputerDetailTab) ? (value as ComputerDetailTab) : 'mcp';
}
