import js from '@eslint/js';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    // 非源码目录交给各自的工具链（Rust / Playwright / 产物）
    ignores: [
      'dist',
      'coverage',
      'playwright-report',
      'test-results',
      'src-tauri',
      'e2e',
      'node_modules',
    ],
  },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: {
      ecmaVersion: 2020,
      globals: { ...globals.browser, ...globals.node },
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      // TS 自身已校验未定义标识符，关闭 core no-undef 避免误报（含 vitest 全局）
      'no-undef': 'off',
      // 尊重代码里 `_`/`_uri` 这类“有意未用”的下划线前缀约定
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
      // 只启用经典 hooks 规则；v7 的 React Compiler 规则暂不强上
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      'react-refresh/only-export-components': [
        'warn',
        { allowConstantExport: true },
      ],
    },
  },
  {
    // 测试文件用 `as any` mock 是惯例，对其放开 no-explicit-any（生产代码仍拦）
    files: ['src/test/**/*.{ts,tsx}', '**/*.test.{ts,tsx}'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },
);
