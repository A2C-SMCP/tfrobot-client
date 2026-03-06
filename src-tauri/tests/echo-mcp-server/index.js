// MCP JSON-RPC over stdio - newline-delimited JSON framing (MCP spec 2025-03-26)
//
// Supported methods:
// - initialize           → returns server info + capabilities
// - notifications/initialized → no-op (notification)
// - tools/list           → returns [{name: "echo", inputSchema: {...}}]
// - tools/call           → echoes back input arguments
// - shutdown             → graceful exit

const readline = require('readline');

const rl = readline.createInterface({ input: process.stdin, terminal: false });

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    const request = JSON.parse(trimmed);
    handleRequest(request);
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
        serverInfo: { name: 'echo-mcp-server', version: '0.1.0' },
        capabilities: { tools: {} },
      };
      break;

    case 'notifications/initialized':
      return; // notification, no response

    case 'tools/list':
      result = {
        tools: [
          {
            name: 'echo',
            description: 'Echoes back the input',
            inputSchema: {
              type: 'object',
              properties: {
                message: { type: 'string', description: 'Message to echo' },
              },
              required: ['message'],
            },
          },
        ],
      };
      break;

    case 'tools/call': {
      const toolName = params?.name;
      const args = params?.arguments || {};
      if (toolName === 'echo') {
        result = {
          content: [
            { type: 'text', text: JSON.stringify(args) },
          ],
        };
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
      // For unknown methods, send error if it has an id (request), ignore if notification
      if (id !== undefined) {
        sendResponse(id, null, { code: -32601, message: `Unknown method: ${method}` });
      }
      return;
  }

  sendResponse(id, result);
}

function sendResponse(id, result, error) {
  if (id === undefined) return; // don't respond to notifications
  const response = {
    jsonrpc: '2.0',
    id,
    ...(error ? { error } : { result }),
  };
  process.stdout.write(JSON.stringify(response) + '\n');
}
