// MCP JSON-RPC over stdio with two tools for alias collision tests.

const readline = require('readline');

const rl = readline.createInterface({ input: process.stdin, terminal: false });

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    handleRequest(JSON.parse(trimmed));
  } catch (e) {
    process.stderr.write(`Parse error: ${e.message}\n`);
  }
});

function handleRequest(req) {
  const { id, method, params } = req;
  let result;

  switch (method) {
    case 'initialize':
      result = {
        protocolVersion: '2025-03-26',
        serverInfo: { name: 'multi-tool-mcp-server', version: '0.1.0' },
        capabilities: { tools: {} },
      };
      break;

    case 'notifications/initialized':
      return;

    case 'tools/list':
      result = {
        tools: [
          {
            name: 'first-tool',
            description: 'First test tool',
            inputSchema: { type: 'object', properties: {} },
          },
          {
            name: 'second-tool',
            description: 'Second test tool',
            inputSchema: { type: 'object', properties: {} },
          },
        ],
      };
      break;

    case 'tools/call': {
      const toolName = params?.name;
      if (toolName === 'first-tool' || toolName === 'second-tool') {
        result = { content: [{ type: 'text', text: toolName }] };
      } else {
        sendResponse(id, null, { code: -32601, message: `Unknown tool: ${toolName}` });
        return;
      }
      break;
    }

    case 'shutdown':
      sendResponse(id, {});
      process.exit(0);
      return;

    default:
      if (id !== undefined) {
        sendResponse(id, null, { code: -32601, message: `Unknown method: ${method}` });
      }
      return;
  }

  sendResponse(id, result);
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
