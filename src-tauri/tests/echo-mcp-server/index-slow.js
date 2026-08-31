// Slow MCP JSON-RPC over stdio server for timeout-path integration tests.

const readline = require('readline');
const fs = require('fs');

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

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function handleRequest(req) {
  const { id, method, params } = req;

  switch (method) {
    case 'initialize':
      if (process.env.START_MARKER_FILE) {
        fs.writeFileSync(process.env.START_MARKER_FILE, 'initializing');
      }
      await sleep(Number(process.env.START_DELAY_MS || 0));
      sendResponse(id, {
        protocolVersion: '2025-03-26',
        serverInfo: { name: 'slow-mcp-server', version: '0.1.0' },
        capabilities: { tools: {} },
      });
      return;

    case 'notifications/initialized':
      return;

    case 'tools/list':
      sendResponse(id, {
        tools: [
          {
            name: 'slow_echo',
            description: 'Echoes after a delay',
            inputSchema: {
              type: 'object',
              properties: {
                message: { type: 'string', description: 'Message to echo' },
                delayMs: { type: 'number', description: 'Delay in milliseconds' },
              },
              required: ['message'],
            },
          },
        ],
      });
      return;

    case 'tools/call': {
      const toolName = params?.name;
      const args = params?.arguments || {};
      if (toolName !== 'slow_echo') {
        sendResponse(id, null, { code: -32601, message: `Unknown tool: ${toolName}` });
        return;
      }
      await sleep(args.delayMs || 1000);
      sendResponse(id, {
        content: [
          { type: 'text', text: JSON.stringify(args) },
        ],
      });
      return;
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
  const response = {
    jsonrpc: '2.0',
    id,
    ...(error ? { error } : { result }),
  };
  process.stdout.write(JSON.stringify(response) + '\n');
}
