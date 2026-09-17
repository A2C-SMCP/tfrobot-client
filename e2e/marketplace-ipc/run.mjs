import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, realpath, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInterface } from 'node:readline';

const root = await realpath(await mkdtemp(join(tmpdir(), 'marketplace-ipc-')));
const repo = join(root, 'source');
const bare = join(root, 'market.git');
await mkdir(join(repo, '.tfrobot-plugin'), { recursive: true });
await mkdir(join(repo, 'plugins/audit/skills/review'), { recursive: true });
await writeFile(join(repo, '.tfrobot-plugin/marketplace.json'), JSON.stringify({ plugins: [{ name: 'audit', source: './plugins/audit' }] }));
await writeFile(join(repo, 'plugins/audit/skills/review/SKILL.md'), '---\nname: review\ndescription: Acceptance fixture\n---\nFixture.\n');
function git(...args) { return execFileSync('git', args, { cwd: repo, stdio: 'pipe' }); }
git('init', '-q');
git('add', '--', '.tfrobot-plugin', 'plugins');
git('-c', 'user.name=Acceptance', '-c', 'user.email=acceptance@example.invalid', 'commit', '-qm', 'fixture');
git('clone', '--bare', repo, bare);
let requests = 0;
const backends = new Set();
// Official Git Smart HTTP backend: supports the SDK's shallow clone over a real socket.
const server = createServer((request, response) => {
  const url = new URL(request.url, 'http://localhost');
  if (!((request.method === 'GET' && url.pathname === '/market.git/info/refs'
      && url.searchParams.get('service') === 'git-upload-pack')
    || (request.method === 'POST' && url.pathname === '/market.git/git-upload-pack'))) {
    response.writeHead(404).end(); return;
  }
  requests++;
  const backend = spawn('git', ['http-backend'], {
    env: { ...process.env, GIT_PROJECT_ROOT: root, GIT_HTTP_EXPORT_ALL: '1',
      PATH_INFO: url.pathname, QUERY_STRING: url.search.slice(1),
      REQUEST_METHOD: request.method, CONTENT_TYPE: request.headers['content-type'] || '',
      CONTENT_LENGTH: request.headers['content-length'] || '',
      GIT_PROTOCOL: request.headers['git-protocol'] || '' },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  backends.add(backend);
  const chunks = [];
  backend.stdout.on('data', chunk => chunks.push(chunk));
  backend.stderr.on('data', data => process.stderr.write(data));
  backend.on('error', () => response.writeHead(500).end());
  backend.stdin.on('error', () => response.destroy());
  request.pipe(backend.stdin);
  backend.on('close', code => {
    backends.delete(backend);
    if (response.destroyed || response.writableEnded) return;
    const output = Buffer.concat(chunks);
    const boundary = output.indexOf('\r\n\r\n');
    if (code !== 0 || boundary < 0) { response.writeHead(500).end(); return; }
    let status = 200;
    for (const line of output.subarray(0, boundary).toString().split('\r\n')) {
      const colon = line.indexOf(':');
      if (colon < 0) continue;
      const name = line.slice(0, colon);
      const value = line.slice(colon + 1).trim();
      if (name.toLowerCase() === 'status') status = Number(value.split(' ')[0]);
      else response.setHeader(name, value);
    }
    response.writeHead(status).end(output.subarray(boundary + 4));
  });
});
await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const target = process.env.CARGO_TARGET_DIR || join(repository, 'src-tauri/target');
let child;
let timer;
try {
  child = spawn(join(target, 'debug/examples/marketplace_ipc_acceptance'), [], {
    env: { ...process.env, MARKETPLACE_IPC_DATA: join(root, 'data'), MARKETPLACE_IPC_LOCAL: repo,
      MARKETPLACE_IPC_REMOTE: `http://127.0.0.1:${server.address().port}/market.git`, GIT_TERMINAL_PROMPT: '0' },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let result;
  createInterface({ input: child.stdout }).on('line', line => {
    if (line.startsWith('ACCEPTANCE:')) result = JSON.parse(line.slice('ACCEPTANCE:'.length));
  });
  child.stderr.on('data', data => process.stderr.write(data));
  const exit = await new Promise((resolve, reject) => {
    timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('Native acceptance timed out after 90 seconds')); }, 90000);
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({ code, signal }));
  });
  const evidence = { ...result, exit, gitHttpRequests: requests };
  await writeFile(join(root, 'result.json'), JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify(evidence, null, 2));
  console.log(`Evidence: ${root}/result.json`);
  assert.equal(exit.code, 0);
  assert.equal(result?.passed, true);
  assert.equal(result.observations.length, 4);
  assert(requests > 0, 'Remote repository must actually be fetched over HTTP');
  console.log(`PASS: ${root}/result.json`);
} finally {
  clearTimeout(timer);
  if (child && child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  for (const backend of backends) backend.kill('SIGKILL');
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
}
