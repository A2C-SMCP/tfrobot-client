import { Button, Select, Space } from 'antd';
import { useTranslation } from 'react-i18next';

interface McpSourceFilterProps {
  options: { value: string; label: string }[];
  selection: string[] | undefined;
  onChange: (value: string[] | undefined) => void;
}

export function McpSourceFilter({ options, selection, onChange }: McpSourceFilterProps) {
  const { t } = useTranslation();
  const available = options.map(({ value }) => value);
  const labelCounts = new Map<string, number>();
  for (const { label } of options) labelCounts.set(label, (labelCounts.get(label) ?? 0) + 1);
  const labeledOptions = options.map(({ value, label }) => ({
    value,
    label: (labelCounts.get(label) ?? 0) > 1 ? `${label} (${value})` : label,
  }));
  const selected = selection === undefined ? available : available.filter((id) => selection.includes(id));
  const label = t('computer.sourceFilter.label');

  return <Select<string[]>
    mode="multiple"
    aria-label={label}
    placeholder={t('computer.sourceFilter.none')}
    value={selected}
    maxTagCount={0}
    maxTagPlaceholder={selection === undefined
      ? t('computer.sourceFilter.all')
      : t('computer.sourceFilter.selected', { count: selected.length })}
    onChange={(next) => onChange(next.length === available.length ? undefined : next)}
    options={labeledOptions}
    optionFilterProp="label"
    style={{ width: 210, maxWidth: '100%' }}
    popupRender={(menu) => <>
      <Space size={4} onMouseDown={(event) => event.preventDefault()} style={{ padding: '4px 8px' }}>
        <Button size="small" type="link" onClick={() => onChange(undefined)}>{t('computer.sourceFilter.selectAll')}</Button>
        <Button size="small" type="link" onClick={() => onChange([])}>{t('computer.sourceFilter.clear')}</Button>
      </Space>
      {menu}
    </>}
  />;
}
