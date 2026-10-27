import { test } from 'node:test';
import assert from 'node:assert/strict';
import { HostSqliteDatabase, encode, decodeRow } from './src/storage.ts';

function fixture() {
  const calls = [];
  const host = Object.fromEntries(['exec', 'run', 'get', 'all', 'begin', 'commit', 'rollback']
    .map(method => [method, (...args) => { calls.push([method, ...args]); }]));
  host.all = () => '[]';
  return { host, calls, db: new HostSqliteDatabase(host) };
}
test('typed values preserve s64, text, null, real and binary', async () => {
  const values = [null, 1.25, 42, 9007199254740993n, '🌻', new Uint8Array([0, 255, 128])];
  const { db, calls, host } = fixture();
  await db.run('insert', ...values);
  assert.deepEqual(calls[0][2], values.map(encode));
  const tagged = Object.fromEntries(values.map((v, i) => {
    const wire = encode(v);
    return [i, wire.tag === 'integer' ? { tag: 'integer', val: String(wire.val) }
      : wire.tag === 'blob' ? { tag: 'blob', val: [...wire.val] } : wire];
  }));
  assert.deepEqual(Object.values(decodeRow(JSON.stringify(tagged))), values);
  host.get = () => JSON.stringify(tagged);
  host.all = () => JSON.stringify([tagged]);
  assert.deepEqual(Object.values(await db.get('select')), values);
  assert.deepEqual((await db.all('select')).map(Object.values), [values]);
  host.get = () => undefined;
  assert.equal(await db.get('missing'), undefined);
});
test('database methods inside callback never settle; tx works and expires', async () => {
  const { db, calls } = fixture();
  let handle;
  const pending = [];
  await db.transaction(async tx => {
    handle = tx;
    pending.push(db.exec('bad'), db.run('bad'), db.get('bad'), db.all('bad'),
      db.transaction(async () => { throw new Error('must not run'); }), db.close());
    await tx.run('good', 7);
  });
  assert.deepEqual(calls.map(c => c[0]), ['begin', 'run', 'commit']);
  assert.equal(calls[1][1], 'good');
  const timeout = new Promise(resolve => setTimeout(() => resolve('pending'), 20));
  assert.equal(await Promise.race([Promise.race(pending).then(() => 'settled'), timeout]), 'pending');
  await assert.rejects(handle.run('expired'), /no longer active/);
  await db.exec('outside');
  assert.equal(calls.at(-1)[1], 'outside');
});
test('rollback, including rollback failure', async () => {
  const { db, calls, host } = fixture();
  const failure = new Error('callback failed');
  await assert.rejects(db.transaction(async () => { throw failure; }), e => e === failure);
  assert.deepEqual(calls.map(c => c[0]), ['begin', 'rollback']);
  host.rollback = () => { throw new Error('disk failed'); };
  await assert.rejects(db.transaction(async () => { throw failure; }), AggregateError);
  assert.equal(db.inCallback, false);
});
