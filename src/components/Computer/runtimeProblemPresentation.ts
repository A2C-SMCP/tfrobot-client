import type { TFunction } from 'i18next';
import type { ComputerRuntimeAffectedCapability } from '@/stores/runtimeSnapshot';

export function affectedCapabilityLabel(
  capability: ComputerRuntimeAffectedCapability,
  t: TFunction,
): string {
  switch (capability.kind) {
    case 'runtime':
      return t('computer.runtime.problems.capabilities.runtime');
    case 'connection':
      return t('computer.runtime.problems.capabilities.connection');
    case 'mcp_server':
      return t('computer.runtime.problems.capabilities.mcpServer', {
        name: capability.name ?? capability.bundle_id,
      });
  }
}
