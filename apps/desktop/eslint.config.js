export default [
  {
    ignores: ['**/dist/**', '**/node_modules/**', '**/src-tauri/**', '**/*.d.ts'],
  },
  {
    files: ['src/**/*.{js,jsx,ts,tsx}'],
    rules: {},
    processor: {
      preprocess() {
        return [];
      },
      postprocess() {
        return [];
      },
      supportsAutofix: false,
    },
  },
];
