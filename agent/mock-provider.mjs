// Optional PR4 check: real pi-ai OpenRouter provider, ONLY a local mock.
// This Node check is separate from the Wasm on-stack fetch/abort verdict.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { performance } from 'node:perf_hooks';
import { createModels } from '@earendil-works/pi-ai/models';
import { openrouterProvider } from '@earendil-works/pi-ai/providers/openrouter';

const server = createServer(async (req, res) => {
  assert.equal(req.url, '/chat/completions');
  assert.equal(req.headers.authorization, 'Bearer local-mock-only');
  for await (const _ of req) { /* drain the local request, never log it */ }
  res.writeHead(200, { 'Content-Type': 'text/event-stream' });
  for (let i = 0; i < 12; i++) {
    res.write(`data: ${JSON.stringify({ id: 'mock', object: 'chat.completion.chunk',
      choices: [{ index: 0, delta: { content: `token${i} ` }, finish_reason: null }] })}\n\n`);
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  res.end(`data: ${JSON.stringify({ id: 'mock', object: 'chat.completion.chunk',
    choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] })}\n\ndata: [DONE]\n\n`);
});
server.listen(0, '127.0.0.1');
await once(server, 'listening');
try {
  const models = createModels({
    credentials: {
      read: async () => ({ type: 'api_key', key: 'local-mock-only' }),
      list: async () => [], modify: async () => undefined, delete: async () => {},
    },
    // Explicitly prohibit ambient credentials and filesystem probes.
    authContext: { env: async () => undefined, fileExists: async () => false },
  });
  const provider = openrouterProvider();
  models.setProvider(provider);
  const model = { ...models.getModels('openrouter').find(m => m.api === 'openai-completions'),
    baseUrl: `http://127.0.0.1:${server.address().port}` };
  assert.ok(model.id);
  const start = performance.now();
  const stamps = [];
  for await (const event of models.stream(model, {
    messages: [{ role: 'user', content: 'local mock test', timestamp: Date.now() }],
  })) {
    if (event.type === 'error') throw new Error(event.error.errorMessage);
    if (event.type === 'text_delta') {
      stamps.push(performance.now() - start);
      console.log(`PI_AI_MOCK delta=${stamps.length} ms=${stamps.at(-1).toFixed(3)}`);
    }
  }
  assert.equal(stamps.length, 12);
  assert.ok(stamps.at(-1) - stamps[0] > 900);
  console.log('PI_AI_MOCK PASS (Node provider only; no real OpenRouter request)');
} finally {
  server.closeAllConnections();
  server.close();
}
