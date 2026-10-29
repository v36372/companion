import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { test } from 'node:test';

test('mock streams a diagram and fails only the latest fail prompt', async () => {
  const child = spawn(process.execPath, [new URL('./mock-chat-server.mjs', import.meta.url).pathname], {
    env: {...process.env, PORT: '0'}, stdio: ['ignore', 'pipe', 'inherit'],
  });
  try {
    const base = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.stdout.once('data', data => resolve(data.toString().match(/http:\/\/127\.0\.0\.1:\d+/)[0]));
    });
    const request = (content, path = '/api/v1/chat/completions') => fetch(`${base}${path}`, {method: 'POST', body: JSON.stringify({messages: [
      {role: 'user', content: 'fail earlier'}, {role: 'user', content},
    ]})});
    assert.equal((await request('fail')).status, 500);
    assert.equal((await request('fail', '/chat/completions')).status, 500);
    const start = Date.now();
    const response = await request('draw a list');
    assert.equal(response.status, 200);
    let text = '';
    let chunks = 0;
    for await (const chunk of response.body) { text += new TextDecoder().decode(chunk); chunks++; }
    assert.ok(chunks > 5);
    assert.ok(Date.now() - start > 2500);
    const events = text.split('\n').filter(line => line.startsWith('data: {')).map(line => JSON.parse(line.slice(6)));
    const answer = events.map(event => event.choices[0].delta.content ?? '').join('');
    assert.match(answer, /```svg\n<svg/);
    assert.match(answer, /viewBox=/);
    assert.match(text, /data: \[DONE\]/);
  } finally { child.kill(); }
});
