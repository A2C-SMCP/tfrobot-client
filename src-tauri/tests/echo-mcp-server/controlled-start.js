// Real stdio MCP whose initialize response is released by the test over TCP.
// The controller observes process admission without timing the process spawn.
const net = require('net');
const readline = require('readline');
const [port, name] = process.argv.slice(2);
const controllers = new Set();
const lines = readline.createInterface({ input: process.stdin, terminal: false });

function reply(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\n');
}

lines.on('line', (line) => {
  const { id, method } = JSON.parse(line);
  if (method === 'initialize') {
    const controller = net.connect(Number(port), '127.0.0.1', () => {
      controller.write(name + '\n');
    });
    controllers.add(controller);
    controller.once('data', () => {
      reply(id, {
        protocolVersion: '2025-03-26',
        serverInfo: { name, version: '1.0.0' },
        capabilities: { tools: {} },
      });
      controller.end();
    });
    controller.on('error', () => process.exit(1));
    controller.on('close', () => controllers.delete(controller));
  } else if (method === 'tools/list') {
    reply(id, { tools: [] });
  } else if (id !== undefined) {
    reply(id, {});
  }
});

lines.on('close', () => {
  for (const controller of controllers) controller.destroy();
});
