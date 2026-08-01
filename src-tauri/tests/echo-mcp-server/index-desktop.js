// MCP JSON-RPC over stdio fixture for Desktop Resources lazy-loading integration tests.

const fs = require('fs');
const readline = require('readline');

const rl = readline.createInterface({ input: process.stdin, terminal: false });
const markerPath = process.argv[2];

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    handleRequest(JSON.parse(trimmed));
  } catch (error) {
    process.stderr.write(`Parse error: ${error.message}\n`);
  }
});

function handleRequest(request) {
  const { id, method, params } = request;

  switch (method) {
    case 'initialize':
      sendResponse(id, {
        protocolVersion: '2025-03-26',
        serverInfo: { name: 'desktop-resource-fixture', version: '0.1.0' },
        capabilities: { resources: {} },
      });
      return;
    case 'notifications/initialized':
      return;
    case 'resources/list':
      sendResponse(id, {
        resources: [{
          uri: 'window://fixture/main',
          name: 'Fixture Window',
          description: 'A real MCP resource used to verify lazy reads',
          mimeType: 'text/plain',
        }],
      });
      return;
    case 'resources/read':
      if (markerPath) fs.appendFileSync(markerPath, `${params?.uri || 'unknown'}\n`);
      sendResponse(id, {
        contents: [{
          uri: params?.uri,
          mimeType: 'text/plain',
          text: 'Fixture window content',
        }],
      });
      return;
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
  const response = {
    jsonrpc: '2.0',
    id,
    ...(error ? { error } : { result }),
  };
  process.stdout.write(`${JSON.stringify(response)}\n`);
}
