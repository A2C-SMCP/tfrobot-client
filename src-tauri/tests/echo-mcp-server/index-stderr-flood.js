// MCP JSON-RPC over stdio - stderr flood variant for Issue #19 regression test.
//
// Identical to index.js but uses synchronous (blocking) writes to stderr fd
// on startup and on every tools/call request.  This reproduces the pipe-buffer
// deadlock described in https://github.com/A2C-SMCP/tfrobot-client/issues/19:
//
//   stderr pipe never consumed → 64 KB buffer fills → child write() blocks
//   → async event loop stalls → Socket.IO / tool calls time out.
//
// We use fs.writeSync(2, ...) instead of process.stderr.write() because the
// latter is buffered by libuv and won't actually block the Node.js process
// even when the kernel pipe buffer is full.  fs.writeSync(2, ...) issues a
// real write(2) syscall that WILL block when the pipe buffer is full.
//
// Supported methods: same as index.js (initialize, tools/list, tools/call, shutdown).

const fs = require('fs');
const readline = require('readline');

const rl = readline.createInterface({ input: process.stdin, terminal: false });

// ── Flood stderr on startup using SYNCHRONOUS writes (>64 KB to overflow pipe buffer) ──
// 2000 lines × ~80 bytes ≈ 160 KB, well over the typical 64 KB pipe buffer.
const FLOOD_LINES = 2000;
for (let i = 0; i < FLOOD_LINES; i++) {
  fs.writeSync(2, `[stderr-flood] startup log line ${i}: ${'x'.repeat(60)}\n`);
}

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  try {
    const request = JSON.parse(trimmed);
    handleRequest(request);
  } catch (e) {
    fs.writeSync(2, `Parse error: ${e.message}\n`);
  }
});

function handleRequest(req) {
  const { id, method, params } = req;

  let result;

  switch (method) {
    case 'initialize':
      result = {
        protocolVersion: '2025-03-26',
        serverInfo: { name: 'echo-mcp-server-stderr-flood', version: '0.1.0' },
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
      // Extra synchronous stderr on every tool call
      for (let i = 0; i < 500; i++) {
        fs.writeSync(2, `[stderr-flood] tool-call log line ${i}: ${'y'.repeat(60)}\n`);
      }

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
  process.stdout.write(JSON.stringify(response) + '\n');
}
