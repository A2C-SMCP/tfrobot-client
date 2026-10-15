import type { ThemeConfig } from 'antd';
import { theme } from 'antd';

/** Shared desktop chrome and component tokens, including themed portal surfaces. */
export function clientTheme(dark: boolean): ThemeConfig {
  return {
    cssVar: { key: 'client' },
    algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      colorPrimary: dark ? '#0a84ff' : '#007aff',
      colorBgLayout: dark ? '#1c1c1e' : '#fafafa',
      colorBgContainer: dark ? '#242426' : '#ffffff',
      colorBgElevated: dark ? '#2b2b2f' : '#ffffff',
      colorText: dark ? '#f2f2f7' : '#202124',
      colorTextSecondary: dark ? '#aaaab1' : '#6e7076',
      colorBorder: dark ? '#48484f' : '#d4d4db',
      colorBorderSecondary: dark ? '#39393e' : '#e6e6eb',
      colorFillAlter: dark ? '#2b2b2f' : '#f3f3f5',
      colorInfoBg: dark ? '#2b2b2f' : '#f3f3f5',
      colorInfoBorder: dark ? '#39393e' : '#e6e6eb',
      colorSuccess: dark ? '#77c899' : '#26804b',
      colorError: dark ? '#ff7770' : '#c53c36',
      borderRadius: 7,
      borderRadiusLG: 12,
      fontFamily: "-apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Helvetica Neue', Arial, sans-serif",
      fontSize: 13,
      fontSizeHeading3: 28,
      fontSizeHeading4: 26,
      fontSizeHeading5: 16,
      controlHeight: 32,
      boxShadowSecondary: dark ? '0 8px 30px #00000045' : '0 8px 30px #161b2518',
    },
    components: {
      Layout: {
        headerBg: dark ? '#1c1c1e' : '#fafafa',
        siderBg: dark ? '#292a2e' : '#eff0f3',
        headerHeight: 54,
      },
      Menu: {
        itemBg: 'transparent',
        itemHeight: 36,
        itemBorderRadius: 7,
        itemSelectedBg: dark ? '#0a84ff' : '#007aff',
        itemSelectedColor: '#ffffff',
        groupTitleColor: dark ? '#aaaab1' : '#6e7076',
      },
      Button: { primaryShadow: '0 1px 2px #007aff20', fontWeight: 500 },
      Card: { headerFontSize: 14, headerHeight: 48, bodyPadding: 20, headerPadding: 20 },
      Table: { headerBg: dark ? '#2b2b2f' : '#f3f3f5', headerColor: dark ? '#aaaab1' : '#6e7076' },
      Modal: { borderRadiusLG: 15, titleFontSize: 20 },
      Statistic: { contentFontSize: 25, titleFontSize: 12 },
    },
  };
}
