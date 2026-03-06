import { info, warn, error, debug, attachConsole } from '@tauri-apps/plugin-log';

export { info, warn, error, debug };

export async function initLogger() {
  if (import.meta.env.DEV) {
    await attachConsole();
  }
}
