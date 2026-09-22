import { useEffect, useRef, useState } from 'react';
import { Card, Space, Typography, theme } from 'antd';
import { useTranslation } from 'react-i18next';
import {
  PERMISSION_ANCHORS,
  permissionAnchorId,
  type PermissionAnchor,
} from './permissions';

const { Paragraph, Text, Title } = Typography;

/** How long an anchor arriving from a notice stays highlighted. */
const HIGHLIGHT_DURATION_MS = 2400;

/** Body copy for each anchor lives in the existing `permissions.*` keys. */
const ANCHOR_BODY_KEYS: Record<PermissionAnchor, string> = {
  mcp: 'permissions.mcp',
  password: 'permissions.password',
  purpose: 'permissions.purpose',
  migration: 'permissions.migration',
  update: 'permissions.update',
  paused: 'permissions.description',
};

interface PermissionsSettingsProps {
  /** Anchor a notice asked to open; the page scrolls to it and marks it briefly. */
  focusAnchor?: PermissionAnchor | null;
}

/**
 * The single authoritative place for system-permission copy. Dismissible notices point at these
 * anchors, so a user who turned a notice off can still read the full explanation afterwards.
 */
export function PermissionsSettings({ focusAnchor = null }: PermissionsSettingsProps) {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [highlight, setHighlight] = useState<PermissionAnchor | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!focusAnchor) return undefined;
    const target = rootRef.current?.querySelector(`#${permissionAnchorId(focusAnchor)}`);
    // jsdom has no layout, so the scroll is best effort and the highlight is what tests can see.
    target?.scrollIntoView?.({ block: 'start' });
    setHighlight(focusAnchor);
    const timer = window.setTimeout(() => setHighlight(null), HIGHLIGHT_DURATION_MS);
    return () => window.clearTimeout(timer);
  }, [focusAnchor]);

  return (
    <div ref={rootRef}>
      <Space direction="vertical" size={12} style={{ width: '100%' }}>
        <Paragraph type="secondary" style={{ marginBottom: 0 }}>
          {t('settings.permissionsPage.intro')}
        </Paragraph>
        {PERMISSION_ANCHORS.map((anchor) => (
          <div
            key={anchor}
            id={permissionAnchorId(anchor)}
            data-focused={highlight === anchor ? 'true' : undefined}
            style={{
              borderRadius: token.borderRadius,
              background: highlight === anchor ? token.colorPrimaryBg : undefined,
            }}
          >
            <Card size="small" title={<Title level={5} style={{ margin: 0 }}>{t(`settings.permissionsPage.items.${anchor}`)}</Title>}>
              <Text>{t(ANCHOR_BODY_KEYS[anchor])}</Text>
            </Card>
          </div>
        ))}
      </Space>
    </div>
  );
}
