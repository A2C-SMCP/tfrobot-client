import { execFileSync } from 'node:child_process';

const manifestPath = 'src-tauri/Cargo.toml';
const metadata = JSON.parse(execFileSync(
  'cargo',
  ['metadata', '--locked', '--format-version=1', '--manifest-path', manifestPath],
  { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 },
));
const sdk = metadata.packages.find((pkg) => pkg.name === 'a2c-smcp');
const sdkPackageNames = new Set([
  'a2c-smcp',
  'smcp',
  'smcp-client-transport',
  'smcp-computer',
]);
const gitSdkPackages = metadata.packages.filter((pkg) => (
  sdkPackageNames.has(pkg.name) && pkg.source?.startsWith('git+')
));

if (gitSdkPackages.length > 0) {
  throw new Error(
    `A2C-SMCP packages must not resolve from git: ${gitSdkPackages.map((pkg) => pkg.name).join(', ')}`,
  );
}

if (!sdk || sdk.version !== '0.4.1' || !sdk.source?.startsWith('registry+')) {
  throw new Error(
    `a2c-smcp must resolve to crates.io 0.4.1; got ${sdk ? `${sdk.version} from ${sdk.source}` : 'no package'}`,
  );
}

console.log(`a2c-smcp ${sdk.version} resolves from the registry`);
