import { createHash } from 'node:crypto';
import { createReadStream, existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, readdir, readlink, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';

const PYTHON_VERSION = '3.12.14';
const PYTHON_RELEASE = '20260825';
const TFBASH_VERSION = '0.1.0';
const RESOLUTION_CUTOFF = '2026-08-27T00:00:00Z';
const POWERSHELL_VERSION = '7.6.5';

const targets = {
  'aarch64-apple-darwin': {
    pythonSha256: '8b0f1fa71eab7ca644e482c631807a1116fa848491051cd1c8d9429491de63a6',
    uvPlatform: 'aarch64-apple-darwin',
  },
  'x86_64-apple-darwin': {
    pythonSha256: 'bd486eadd20259ad1fece28c800205baac0113c3b9cc663ddae495c19ba9db38',
    uvPlatform: 'x86_64-apple-darwin',
  },
  'x86_64-unknown-linux-gnu': {
    pythonSha256: '7ce4a71285d913955a76053cc7605ea96da8ecada54dba9cf395245961816421',
    uvPlatform: 'x86_64-unknown-linux-gnu',
  },
  'x86_64-pc-windows-msvc': {
    pythonSha256: '8e6aad12ef6fc9685e67ce66253f8f72d6e8fa02cb7187e5850bd4db5ecd9e2a',
    uvPlatform: 'x86_64-pc-windows-msvc',
    powershellSha256: '32eb8f6cdce08f86e987d625a2733e54ac3e289ae7e1621b14c0b5bcec2434ea',
  },
};

const args = process.argv.slice(2);
const verifyOnly = args.includes('--verify');
const requestedTarget = args.find((value) => !value.startsWith('--'))
  ?? process.env.TAURI_ENV_TARGET_TRIPLE
  ?? process.env.TARGET;
if (!requestedTarget || !(requestedTarget in targets)) {
  throw new Error(`usage: node scripts/prepare-tfbash-runtime.mjs <${Object.keys(targets).join('|')}> [--verify]`);
}

const target = targets[requestedTarget];
const outputRoot = resolve('src-tauri', 'resources', 'tfbash', requestedTarget);
const manifestPath = join(outputRoot, 'runtime-manifest.json');
const lockPath = resolve('scripts', 'tfbash-locks', `${requestedTarget}.txt`);
const pythonPath = requestedTarget.includes('windows')
  ? join(outputRoot, 'python', 'python.exe')
  : join(outputRoot, 'python', 'bin', 'python3');
const packagePath = requestedTarget.includes('windows')
  ? join(outputRoot, 'python', 'Lib', 'site-packages', 'tfbash_mcp', '__init__.py')
  : join(outputRoot, 'python', 'lib', 'python3.12', 'site-packages', 'tfbash_mcp', '__init__.py');
const powershellPath = join(outputRoot, 'powershell', 'pwsh.exe');

async function hashFile(path) {
  const digest = createHash('sha256');
  for await (const chunk of createReadStream(path)) digest.update(chunk);
  return digest.digest('hex');
}

async function hashPayloadTree(root) {
  const digest = createHash('sha256');
  async function visit(directory, prefix = '') {
    const entries = await readdir(directory, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (relativePath === 'runtime-manifest.json') continue;
      const absolutePath = join(directory, entry.name);
      if (entry.isDirectory()) {
        digest.update(`directory\0${relativePath}\0`);
        await visit(absolutePath, relativePath);
      } else if (entry.isSymbolicLink()) {
        digest.update(`symlink\0${relativePath}\0${await readlink(absolutePath)}\0`);
      } else if (entry.isFile()) {
        digest.update(`file\0${relativePath}\0`);
        for await (const chunk of createReadStream(absolutePath)) digest.update(chunk);
        digest.update('\0');
      }
    }
  }
  await visit(root);
  return digest.digest('hex');
}

if (!existsSync(lockPath)) throw new Error(`missing hashed dependency lock: ${lockPath}`);
const dependencyLockSha256 = await hashFile(lockPath);
const expectedManifestBase = {
  schemaVersion: 2,
  target: requestedTarget,
  python: { version: PYTHON_VERSION, release: PYTHON_RELEASE, sha256: target.pythonSha256 },
  tfbashMcp: {
    version: TFBASH_VERSION,
    resolutionCutoff: RESOLUTION_CUTOFF,
    dependencyLockSha256,
  },
  ...(target.powershellSha256 ? {
    powershell: { version: POWERSHELL_VERSION, sha256: target.powershellSha256 },
  } : {}),
};

async function isPrepared() {
  if (!existsSync(manifestPath) || !existsSync(pythonPath) || !existsSync(packagePath)) return false;
  if (target.powershellSha256 && !existsSync(powershellPath)) return false;
  const actual = JSON.parse(await readFile(manifestPath, 'utf8'));
  const { payloadSha256, ...actualBase } = actual;
  if (JSON.stringify(actualBase) !== JSON.stringify(expectedManifestBase)) return false;
  return typeof payloadSha256 === 'string'
    && payloadSha256 === await hashPayloadTree(outputRoot);
}

if (await isPrepared()) {
  console.log(`tfbash runtime verified: ${requestedTarget}`);
  process.exit(0);
}
if (verifyOnly) {
  throw new Error(`tfbash runtime is missing or stale for ${requestedTarget}`);
}

async function run(command, commandArgs, options = {}) {
  await new Promise((resolvePromise, reject) => {
    const child = spawn(command, commandArgs, { stdio: 'inherit', ...options });
    child.on('error', reject);
    child.on('exit', (code) => {
      if (code === 0) resolvePromise();
      else reject(new Error(`${command} exited with code ${code}`));
    });
  });
}

async function download(url, destination, expectedSha256) {
  await run(process.env.CURL ?? 'curl', [
    '--fail', '--location', '--retry', '3', '--output', destination, url,
  ]);
  const actual = await hashFile(destination);
  if (actual !== expectedSha256) {
    throw new Error(`SHA-256 mismatch for ${url}: expected ${expectedSha256}, got ${actual}`);
  }
}

const temporaryRoot = await mkdtemp(join(tmpdir(), 'tfrobot-tfbash-'));
try {
  const pythonAsset = `cpython-${PYTHON_VERSION}+${PYTHON_RELEASE}-${requestedTarget}-install_only_stripped.tar.gz`;
  const pythonArchive = join(temporaryRoot, pythonAsset);
  await download(
    `https://github.com/astral-sh/python-build-standalone/releases/download/${PYTHON_RELEASE}/${encodeURIComponent(pythonAsset)}`,
    pythonArchive,
    target.pythonSha256,
  );

  await rm(outputRoot, { recursive: true, force: true });
  await mkdir(outputRoot, { recursive: true });
  await run('tar', ['-xzf', pythonArchive, '-C', outputRoot]);

  const sitePackages = requestedTarget.includes('windows')
    ? join(outputRoot, 'python', 'Lib', 'site-packages')
    : join(outputRoot, 'python', 'lib', 'python3.12', 'site-packages');
  await mkdir(sitePackages, { recursive: true });
  await run(process.env.UV ?? 'uv', [
    'pip', 'install',
    '--target', sitePackages,
    '--python-version', '3.12',
    '--python-platform', target.uvPlatform,
    '--only-binary', ':all:',
    '--no-compile',
    '--require-hashes',
    '--requirements', lockPath,
  ]);

  if (target.powershellSha256) {
    const powershellAsset = `PowerShell-${POWERSHELL_VERSION}-win-x64.zip`;
    const powershellArchive = join(temporaryRoot, powershellAsset);
    await download(
      `https://github.com/PowerShell/PowerShell/releases/download/v${POWERSHELL_VERSION}/${powershellAsset}`,
      powershellArchive,
      target.powershellSha256,
    );
    await mkdir(join(outputRoot, 'powershell'), { recursive: true });
    await run('tar', ['-xf', powershellArchive, '-C', join(outputRoot, 'powershell')]);
  }

  const payloadSha256 = await hashPayloadTree(outputRoot);
  await writeFile(manifestPath, `${JSON.stringify({
    ...expectedManifestBase,
    payloadSha256,
  }, null, 2)}\n`);
  if (!await isPrepared()) throw new Error(`prepared tfbash runtime failed verification: ${requestedTarget}`);
  console.log(`tfbash runtime prepared: ${requestedTarget}`);
} finally {
  await rm(temporaryRoot, { recursive: true, force: true });
}
