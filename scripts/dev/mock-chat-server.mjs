// Local-only OpenRouter SSE fixture. Launch with Node, then run the app with
// --features test-mock-provider COMPANION_TEST_PROVIDER_BASE_URL=http://127.0.0.1:4319
import { createServer } from 'node:http';
import { setTimeout } from 'node:timers/promises';

const answer = `A Python list holds an ordered sequence of references. Each slot points to an object; changing a slot does not change the other objects.

\`\`\`svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 680 220" width="680" height="220">
<rect width="680" height="220" fill="white"/>
<defs><marker id="arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0 0L8 4L0 8Z" fill="#5865ab"/></marker></defs>
<g font-family="sans-serif" font-size="14" fill="#25324b" text-anchor="middle">
<text x="340" y="28">A list stores references, in order</text>
<g fill="#edf0ff" stroke="#5865ab"><rect x="80" y="55" width="160" height="48" rx="8"/><rect x="260" y="55" width="160" height="48" rx="8"/><rect x="440" y="55" width="160" height="48" rx="8"/></g>
<text x="160" y="84">slot 0</text><text x="340" y="84">slot 1</text><text x="520" y="84">slot 2</text>
<g stroke="#5865ab" marker-end="url(#arrow)"><path d="M160 106V148"/><path d="M340 106V148"/><path d="M520 106V148"/></g>
<text x="160" y="179">"hello"</text><text x="340" y="179">42</text><text x="520" y="179">another list</text>
</g></svg>
\`\`\`

The arrows are references, not copies of the objects.`;
const server = createServer(async (req, res) => {
  if (req.method !== 'POST' || !['/chat/completions', '/api/v1/chat/completions'].includes(req.url)) {
    res.writeHead(404).end(); return;
  }
  let body = '';
  for await (const chunk of req) {
    body += chunk;
    if (body.length > 1024 * 1024) { res.writeHead(413).end(); return; }
  }
  let input;
  try { input = JSON.parse(body); } catch { res.writeHead(400).end(); return; }
  const last = input.messages?.findLast(message => message.role === 'user');
  if (JSON.stringify(last?.content ?? '').toLowerCase().includes('fail')) {
    res.writeHead(500, {'Content-Type': 'application/json'}).end(JSON.stringify({error: {message: 'Mock provider failure'}})); return;
  }
  res.writeHead(200, {'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache'});
  const emit = (delta, finish_reason = null) => res.write(`data: ${JSON.stringify({id: 'mock-chat', object: 'chat.completion.chunk', choices: [{index: 0, delta, finish_reason}]})}\n\n`);
  emit({role: 'assistant'});
  const chunks = answer.match(/[\s\S]{1,64}/g);
  for (const content of chunks) {
    if (res.destroyed) return;
    emit({content});
    await setTimeout(3000 / chunks.length);
  }
  emit({}, 'stop');
  res.end('data: [DONE]\n\n');
});
server.listen(Number(process.env.PORT ?? 4319), '127.0.0.1', () => {
  console.log(`Mock chat: http://127.0.0.1:${server.address().port}`);
});
