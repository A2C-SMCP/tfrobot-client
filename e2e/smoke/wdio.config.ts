import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Build artifact path (macOS)
const appPath = path.resolve(
  __dirname,
  '../../src-tauri/target/release/bundle/macos/TFRobot Client.app/Contents/MacOS/TFRobot Client'
);

export const config = {
  runner: 'local',
  specs: ['./smoke.test.ts'],
  maxInstances: 1,

  // Connect to tauri-driver running on port 4444
  hostname: '127.0.0.1',
  port: 4444,

  capabilities: [{
    browserName: 'wry',
    'tauri:options': {
      application: appPath,
    },
  }],

  // Disable automatic driver management — we use tauri-driver
  automationProtocol: 'webdriver',

  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    timeout: 60000,
  },

  screenshotPath: './screenshots/',
};
