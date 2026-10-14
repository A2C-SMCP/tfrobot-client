import { settingsNavigationKey } from './tabs';

/**
 * Anchors inside Settings → Permissions & security. Every notice that explains a system permission
 * points here, so the page is the single authoritative place for that copy and the anchors are the
 * only thing call sites need to know.
 */
export const PERMISSION_ANCHORS = [
  'mcp',
  'password',
  'purpose',
  'migration',
  'update',
  'paused',
] as const;

export type PermissionAnchor = (typeof PERMISSION_ANCHORS)[number];

export function permissionAnchorId(anchor: PermissionAnchor): string {
  return `permissions-${anchor}`;
}

export function permissionHelpRoute(anchor: PermissionAnchor): string {
  return settingsNavigationKey('permissions', anchor);
}

export function parsePermissionAnchor(value: string | undefined): PermissionAnchor | null {
  return PERMISSION_ANCHORS.includes(value as PermissionAnchor)
    ? (value as PermissionAnchor)
    : null;
}
