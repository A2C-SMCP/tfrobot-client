import { render, screen } from '../helpers/render';
import { describe, expect, it } from 'vitest';
import { PermissionsSettings } from '@/components/Settings/PermissionsSettings';
import { PERMISSION_ANCHORS, permissionAnchorId } from '@/components/Settings/permissions';
import i18n from '@/i18n';

describe('PermissionsSettings', () => {
  it('gives every permission topic its own anchor and the full explanation', () => {
    render(<PermissionsSettings />);

    for (const anchor of PERMISSION_ANCHORS) {
      expect(document.getElementById(permissionAnchorId(anchor))).not.toBeNull();
    }
    expect(screen.getByText(i18n.t('permissions.mcp'))).toBeInTheDocument();
    expect(screen.getByText(i18n.t('permissions.password'))).toBeInTheDocument();
    expect(screen.getByText(i18n.t('permissions.purpose'))).toBeInTheDocument();
    expect(screen.getByText(i18n.t('permissions.migration'))).toBeInTheDocument();
    expect(screen.getByText(i18n.t('permissions.update'))).toBeInTheDocument();
    expect(screen.getByText(i18n.t('permissions.description'))).toBeInTheDocument();
  });

  it('marks only the anchor a notice asked to open', () => {
    render(<PermissionsSettings focusAnchor="password" />);

    expect(document.getElementById('permissions-password')).toHaveAttribute('data-focused', 'true');
    expect(document.getElementById('permissions-mcp')).not.toHaveAttribute('data-focused');
  });

  it('focuses the anchor again when a notice hops to the same topic twice', () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;

    const view = render(<PermissionsSettings focusAnchor="mcp" focusRevision={1} />);
    expect(scrollIntoView).toHaveBeenCalledTimes(1);

    view.rerender(<PermissionsSettings focusAnchor="mcp" focusRevision={2} />);
    expect(scrollIntoView).toHaveBeenCalledTimes(2);
  });
});
