import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dummyCredentials } from './src/credentials.ts';
test('dummy store advertises providers, never persists or returns writes', async () => {
  for (const { providerId } of await dummyCredentials.list()) {
    const expected = { type: 'api_key', key: 'guest-dummy-not-a-secret' };
    assert.deepEqual(await dummyCredentials.read(providerId), expected);
    assert.deepEqual(await dummyCredentials.modify(providerId, async () => ({ type: 'api_key', key: 'must-not-persist' })), expected);
    await dummyCredentials.delete(providerId);
    assert.deepEqual(await dummyCredentials.read(providerId), expected);
  }
  assert.equal(await dummyCredentials.read('unknown'), undefined);
});
