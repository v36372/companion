import { componentize } from '@bytecodealliance/componentize-js';
import { readFile, writeFile } from 'node:fs/promises';

// 0.22.0 has no "cli" or "filesystem" disable feature. Keep its CLI
// imports: the embedder supplies an EMPTY WasiCtx (no env, args or preopens).
// Never forward process.env to Wizer or to the runtime guest.
const { component } = await componentize(await readFile('dist/guest.js', 'utf8'), {
  witWorld: await readFile('../wit/world.wit', 'utf8'), worldName: 'guest',
  enableFeatures: ['clocks', 'http', 'random'],
  disableFeatures: ['fetch-event'], env: {},
});
await writeFile('../crates/companion-agent-host/src/agent-core.wasm', component);
