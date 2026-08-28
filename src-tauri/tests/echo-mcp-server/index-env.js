// MCP JSON-RPC over stdio fixture that exposes selected process environment values.

const readline = require('readline');

const rl = readline.createInterface({ input: process.stdin, terminal: false });

rl.on('line', (line) => {
  const request = JSON.parse(line);
  const { id, method, params } = request;
  if (method === 'notifications/initialized') return;

  let result;
  if (method === 'initialize') {
    result = {
      protocolVersion: '2025-03-26',
      serverInfo: { name: 'env-mcp-server', version: '0.1.0' },
      capabilities: { tools: {} },
    };
  } else if (method === 'tools/list') {
    result = {
      tools: [{
        name: 'read_env',
        description: 'Returns selected process environment values',
        inputSchema: { type: 'object', properties: {} },
      }],
    };
  } else if (method === 'tools/call' && params?.name === 'read_env') {
    result = {
      content: [{
        type: 'text',
        text: JSON.stringify({ LOG_LEVEL: process.env.LOG_LEVEL, REGION: process.env.REGION }),
      }],
    };
  } else if (method === 'shutdown') {
    result = {};
  } else {
    send(id, undefined, { code: -32601, message: `Unknown method: ${method}` });
    return;
  }

  send(id, result);
  if (method === 'shutdown') process.exit(0);
});

function send(id, result, error) {
  process.stdout.write(`${JSON.stringify({
    jsonrpc: '2.0',
    id,
    ...(error ? { error } : { result }),
  })}\n`);
}
