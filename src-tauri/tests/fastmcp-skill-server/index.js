const readline = require('readline');

const skillMd = `---
name: fastmcp-demo
description: FastMCP style demo skill
---

# FastMCP Demo

This skill is exposed as skill://fastmcp-demo/SKILL.md.
`;

const manifest = JSON.stringify({
  skill: 'fastmcp-demo',
  files: [
    { path: 'SKILL.md', size: Buffer.byteLength(skillMd), hash: 'sha256:test-skill-md' },
    { path: 'reference.md', size: 20, hash: 'sha256:test-reference' },
  ],
});

const resources = [
  {
    uri: 'skill://fastmcp-demo/SKILL.md',
    name: 'fastmcp-demo/SKILL.md',
    description: 'Main skill instructions',
    mimeType: 'text/markdown',
  },
  {
    uri: 'skill://fastmcp-demo/_manifest',
    name: 'fastmcp-demo/_manifest',
    description: 'Skill file manifest',
    mimeType: 'application/json',
  },
  {
    uri: 'skill://fastmcp-demo/reference.md',
    name: 'fastmcp-demo/reference.md',
    description: 'Supporting documentation',
    mimeType: 'text/markdown',
  },
];

const rl = readline.createInterface({ input: process.stdin, terminal: false });

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    handleRequest(JSON.parse(trimmed));
  } catch (error) {
    process.stderr.write(`Parse error: ${error.message}\n`);
  }
});

function handleRequest(req) {
  const { id, method, params } = req;

  switch (method) {
    case 'initialize':
      return sendResponse(id, {
        protocolVersion: '2025-03-26',
        serverInfo: { name: 'fastmcp-skill-server', version: '0.1.0' },
        capabilities: { resources: {} },
      });

    case 'notifications/initialized':
      return;

    case 'resources/list':
      return sendResponse(id, { resources });

    case 'resources/read': {
      const uri = params?.uri;
      if (uri === 'skill://fastmcp-demo/SKILL.md') {
        return sendResponse(id, {
          contents: [{ uri, mimeType: 'text/markdown', text: skillMd }],
        });
      }
      if (uri === 'skill://fastmcp-demo/_manifest') {
        return sendResponse(id, {
          contents: [{ uri, mimeType: 'application/json', text: manifest }],
        });
      }
      if (uri === 'skill://fastmcp-demo/reference.md') {
        return sendResponse(id, {
          contents: [{ uri, mimeType: 'text/markdown', text: '# Reference\n' }],
        });
      }
      return sendResponse(id, null, { code: -32002, message: `Unknown resource: ${uri}` });
    }

    case 'shutdown':
      sendResponse(id, {});
      process.exit(0);
      return;

    default:
      if (id !== undefined) {
        sendResponse(id, null, { code: -32601, message: `Unknown method: ${method}` });
      }
  }
}

function sendResponse(id, result, error) {
  if (id === undefined) return;
  process.stdout.write(JSON.stringify({
    jsonrpc: '2.0',
    id,
    ...(error ? { error } : { result }),
  }) + '\n');
}
