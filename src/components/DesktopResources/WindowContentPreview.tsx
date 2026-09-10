import { PageImage as Image } from '@/components/Navigation/PageOverlays';
import { Spin, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type { WindowContent } from '@/stores/desktopStore';

const { Text } = Typography;

const MAX_TEXT_PREVIEW_LENGTH = 500;
const MAX_IMAGE_PREVIEW_BYTES = 2 * 1024 * 1024;
const SAFE_IMAGE_MIME_TYPES = new Set([
  'image/gif',
  'image/jpeg',
  'image/png',
  'image/webp',
]);

function estimatedBase64Bytes(value?: string): number {
  if (!value) return 0;
  const padding = value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0;
  return Math.max(0, Math.floor((value.length * 3) / 4) - padding);
}

function normalizedMimeType(value?: string): string {
  return value?.split(';', 1)[0].trim().toLowerCase() || '';
}

interface WindowContentPreviewProps {
  contents: WindowContent[];
}

export function WindowContentPreview({ contents }: WindowContentPreviewProps) {
  const { t } = useTranslation();

  if (contents.length === 0) {
    return <Text type="secondary">{t('desktop.noContent')}</Text>;
  }

  return contents.map((content, index) => {
    const mimeType = normalizedMimeType(content.mime_type);
    const blobBytes = estimatedBase64Bytes(content.blob);
    const safeImage = content.type === 'blob'
      && SAFE_IMAGE_MIME_TYPES.has(mimeType)
      && blobBytes <= MAX_IMAGE_PREVIEW_BYTES
      && Boolean(content.blob);

    return (
      <div key={`${content.uri}:${index}`} style={{ marginBottom: 8 }}>
        {content.type === 'text' && content.text !== undefined && (
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {content.mime_type || t('desktop.textContent')}
            </Text>
            <div
              style={{
                backgroundColor: '#f5f5f5',
                padding: '8px 12px',
                borderRadius: 4,
                marginTop: 4,
                maxWidth: '100%',
                overflow: 'auto',
              }}
            >
              <Text
                code
                style={{
                  wordBreak: 'break-all',
                  whiteSpace: 'pre-wrap',
                  fontSize: 12,
                }}
              >
                {content.text.length > MAX_TEXT_PREVIEW_LENGTH
                  ? `${content.text.slice(0, MAX_TEXT_PREVIEW_LENGTH)}…`
                  : content.text}
              </Text>
            </div>
          </div>
        )}
        {safeImage && (
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {content.mime_type}
            </Text>
            <div style={{ marginTop: 8 }}>
              <Image
                src={`data:${mimeType};base64,${content.blob}`}
                alt={t('desktop.windowPreviewAlt')}
                style={{ maxWidth: 400, maxHeight: 300, borderRadius: 4 }}
                placeholder={<Spin />}
              />
            </div>
          </div>
        )}
        {content.type === 'blob' && !safeImage && (
          <div>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {content.mime_type || t('desktop.binaryContent')}
            </Text>
            <div style={{ marginTop: 4 }}>
              <Text type="secondary">
                {blobBytes > MAX_IMAGE_PREVIEW_BYTES
                  ? t('desktop.previewTooLarge', { size: blobBytes })
                  : t('desktop.binarySummary', { size: blobBytes })}
              </Text>
            </div>
          </div>
        )}
      </div>
    );
  });
}
