import type { ReactNode } from 'react';
import { CloseOutlined, InfoCircleOutlined, WarningOutlined } from '@ant-design/icons';
import { Button, theme, Typography } from 'antd';

export interface NoticeBarProps {
  tone?: 'info' | 'warning';
  children: ReactNode;
  /** A single primary next step, e.g. a button that opens the related page. */
  action?: ReactNode;
  /** An optional link into the authoritative help page for this notice. */
  help?: ReactNode;
  /**
   * Dismissing hides the notice permanently, so the label is part of the payload rather than an
   * optional extra: an unlabeled close button would be an accessibility regression.
   */
  dismiss?: { label: string; onClick: () => void };
}

/**
 * Section-level notice: a low-weight alternative to `Alert` for content that explains or warns
 * without blocking the current action. Page-blocking failures stay on `Alert`.
 */
export function NoticeBar({ tone = 'info', children, action, help, dismiss }: NoticeBarProps) {
  const { token } = theme.useToken();
  const palette = tone === 'warning'
    ? { background: token.colorWarningBg, border: token.colorWarningBorder, icon: token.colorWarning }
    : { background: token.colorInfoBg, border: token.colorInfoBorder, icon: token.colorInfo };

  return (
    <div
      role="note"
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        gap: 8,
        padding: '8px 12px',
        background: palette.background,
        border: `1px solid ${palette.border}`,
        borderRadius: token.borderRadius,
      }}
    >
      <span aria-hidden style={{ color: palette.icon, lineHeight: '22px' }}>
        {tone === 'warning' ? <WarningOutlined /> : <InfoCircleOutlined />}
      </span>
      <Typography.Text style={{ flex: 1, minWidth: 0 }}>{children}</Typography.Text>
      {help}
      {action}
      {dismiss && (
        <Button
          type="text"
          size="small"
          icon={<CloseOutlined />}
          aria-label={dismiss.label}
          onClick={dismiss.onClick}
        />
      )}
    </div>
  );
}
