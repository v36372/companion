import { test } from 'node:test';
import assert from 'node:assert/strict';
import { anySignal } from './src/abort-signals.ts';
test('composition stays live until either input aborts, retaining reason', () => {
  const left = new AbortController(), right = new AbortController();
  const signal = anySignal([left.signal, right.signal]);
  assert.equal(signal.aborted, false);
  right.abort('expected reason');
  assert.equal(signal.aborted, true);
  assert.equal(signal.reason, 'expected reason');
  left.abort('later reason');
  assert.equal(signal.reason, 'expected reason');
  assert.equal(anySignal([]).aborted, false);
  assert.equal(anySignal([left.signal]).reason, 'later reason');
});
