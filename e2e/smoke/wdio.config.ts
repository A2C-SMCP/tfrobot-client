import path from 'path';

// Build artifact path (macOS)
const appPath = path.resolve(
  __dirname,
  '../../src-tauri/target/release/bundle/macos/TFRobot.app/Contents/MacOS/TFRobot'
);

export const config = {
  runner: 'local',
  specs: ['./smoke.test.ts'],
  maxInstances: 1,

  capabilities: [{
    browserName: 'wry',
    'tauri:options': {
      application: appPath,
    },
  }],

  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    timeout: 60000,
  },

  screenshotPath: './screenshots/',
};
