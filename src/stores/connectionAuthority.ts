import type { ComputerRuntimeSnapshot } from './runtimeSnapshot';

export interface ConnectionStateSummary {
  url: string;
  office_id: string;
  computer_name: string;
  connected_at: string;
  profile_name: string;
  source_type?: string;
  target_id?: string | null;
  target_name?: string | null;
  employee_id?: number | null;
}

export interface ClientConnectionAuthority {
  present: boolean;
  context: ConnectionStateSummary | null;
  revision: number;
}

interface VersionedClientConnectionAuthority extends ClientConnectionAuthority {
  incarnation: ComputerRuntimeSnapshot['incarnation'];
}

const authorities = new Map<string, VersionedClientConnectionAuthority>();

export function setClientConnectionAuthority(
  instanceId: string,
  present: boolean,
  context: ConnectionStateSummary | null | undefined,
  revision: number,
  runtime: ComputerRuntimeSnapshot,
): void {
  const current = authorities.get(instanceId);
  if (current && (
    runtime.incarnation < current.incarnation
    || (runtime.incarnation === current.incarnation && revision < current.revision)
  )) return;
  authorities.set(instanceId, {
    present,
    context: present ? context ?? null : null,
    incarnation: runtime.incarnation,
    revision,
  });
}

export function getClientConnectionAuthority(
  instanceId: string,
  incarnation?: number,
): ClientConnectionAuthority | undefined {
  const authority = authorities.get(instanceId);
  if (!authority || (incarnation !== undefined && authority.incarnation !== incarnation)) {
    return undefined;
  }
  return {
    present: authority.present,
    context: authority.context,
    revision: authority.revision,
  };
}

export function clearClientConnectionAuthority(instanceId: string): void {
  authorities.delete(instanceId);
}

export function resetClientConnectionAuthorities(): void {
  authorities.clear();
}
