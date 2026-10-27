import { describe, it, expect } from 'vitest';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as os from 'node:os';
import { registerEnvConformance } from '@earendil-works/pi-durable/testing';
import { LineScanner, FileError, type FileInfo } from '@earendil-works/pi-durable/env';
import { BACKGROUND_CONTEXT } from '@earendil-works/chord/context';
import { HostExecutionEnv, type FsHost } from './src/env.ts';

// Fake synchronous WIT host, backed by an isolated temp tree, never bundled into
// the guest. The Rust environment tests separately verify real capability policy.
function fakeHost(root: string): FsHost {
  let next = 1;
  const handles = new Map<number, { fd: number; path: string; lines?: {text: string; terminated: boolean}[] }>();
  const opened = (h: number) => { const value = handles.get(h); if (!value) throw new FileError('invalid', 'closed handle'); return value; };
  const info = (p: string, stat = fs.statSync(p)): FileInfo => ({ name: path.basename(p), path: p, kind: stat.isDirectory() ? 'directory' : 'file', size: stat.size, mtimeMs: stat.mtimeMs });
  const host = {
    envId: () => 'fake-host', absolutePath: (cwd: string, p: string) => path.resolve(cwd, p), joinPath: (parts: string[]) => path.join(...parts),
    readTextFile: (p: string) => fs.readFileSync(p, 'utf8'), readBinaryFile: (p: string) => new Uint8Array(fs.readFileSync(p)),
    writeFile: (p: string, value: Uint8Array) => { fs.mkdirSync(path.dirname(p),{recursive:true}); fs.writeFileSync(p, value); }, appendFile: (p: string, value: Uint8Array) => { fs.mkdirSync(path.dirname(p),{recursive:true}); fs.appendFileSync(p, value); },
    truncateFile: (p: string, size: bigint) => fs.truncateSync(p, Number(size)), flushFile: (_p: string) => {}, renameFile: fs.renameSync,
    fileInfo: info, listDir: (p: string) => fs.readdirSync(p).map(n => info(path.join(p,n))),
    listFiles: (p: string, opts: Parameters<FsHost['listFiles']>[1]) => {
      if (!fs.statSync(p).isDirectory()) throw new FileError('not_directory', 'not a directory');
      const entries: {path: string; kind: string}[] = []; let total = 0; let truncated = false;
      const walk = (dir: string) => {
        for (const name of fs.readdirSync(dir).sort()) {
          if (name === '.git' || (opts.hidden === false && name.startsWith('.')) || opts.exclude.includes(name)) continue;
          const full = path.join(dir,name); const relative = path.relative(p,full); const entry = info(full);
          if (entries.length >= (opts.maxEntries ?? 50000) || total + Buffer.byteLength(relative) + 2 > (opts.maxBytes ?? 4194304)) { truncated = true; return; }
          total += Buffer.byteLength(relative)+2; entries.push({path:relative,kind:entry.kind});
          if (entry.kind === 'directory') walk(full);
          if (truncated) return;
        }
      }; walk(p); return JSON.stringify({entries,truncated});
    },
    exists: fs.existsSync, canonicalPath: fs.realpathSync,
    createDir: (p: string, recursive: boolean) => { fs.mkdirSync(p,{recursive}); }, remove: (p: string, recursive: boolean, force: boolean) => fs.rmSync(p,{recursive,force}),
    createTempDir: (prefix = '') => fs.mkdtempSync(path.join(root,prefix)),
    createTempFile: (prefix = '', suffix = '') => { const p = path.join(root, `${prefix}${next++}${suffix}`); fs.writeFileSync(p,''); return p; },
    openBinary: (p: string) => { if (fs.statSync(p).isDirectory()) throw new FileError('is_directory','directory'); const handle = next++; handles.set(handle,{fd:fs.openSync(p,'r'),path:p}); return handle; },
    pread: (h: number, offset: bigint, length: number) => { const value = Buffer.alloc(length); const n = fs.readSync(opened(h).fd,value,0,length,Number(offset)); return new Uint8Array(value.subarray(0,n)); },
    fstat: (h: number) => { const v = opened(h); return {...info(v.path,fs.fstatSync(v.fd)),size:BigInt(fs.fstatSync(v.fd).size)}; },
    scanLines: (h: number, start: number, end?: number) => { const scan = new LineScanner(start,end); const fd = opened(h).fd; const value = Buffer.alloc(fs.fstatSync(fd).size); fs.readSync(fd,value,0,value.length,0); scan.push(value); return scan.finish(); },
    closeHandle: (h: number) => { const v = handles.get(h); if (v) { fs.closeSync(v.fd); handles.delete(h); } },
    openTextLineReader: (p: string) => {
      const h = host.openBinary(p); const value = opened(h); const text = new TextDecoder().decode(fs.readFileSync(value.fd));
      value.lines = text.split('\n').map((text, i, all) => ({text,terminated:i < all.length-1}));
      if (value.lines.at(-1)?.text === '') value.lines.pop(); return h;
    },
    readLine: (h: number) => opened(h).lines!.shift(),
    readTextLines: (p: string, max = Infinity) => { const h = host.openTextLineReader(p); try { return opened(h).lines!.slice(0,max).map(l => l.text); } finally { host.closeHandle(h); } },
    watchRegister: (_targets: string) => next++, watchClose: (_h: number) => {},
    fsCleanup: () => { for (const h of handles.keys()) host.closeHandle(h); },
    exec: () => { throw { code:'shell_unavailable',message:'no shell' }; },
  };
  // Match WIT thrown structured errors, rather than leaking Node errno strings.
  return new Proxy(host, { get(target, key) { const fn = target[key as keyof typeof target]; return (...args: unknown[]) => {
    try { return (fn as Function)(...args); } catch(e) {
      const value = e as {code?: string;message?: string}; const codes: Record<string,string> = {ENOENT:'not_found',EISDIR:'is_directory',ENOTDIR:'not_directory',EINVAL:'invalid'};
      if (value.code && codes[value.code]) throw {code:codes[value.code],message:value.message}; throw e;
    }
  }; } }) as FsHost;
}
const xfails = new Set([
  "watch reports a missing file's creation, changes, replacement and removal",
  'watch reports a missing target whose ancestors are created',
  'watch follows directories created together with their contents',
  'watch keeps watching a path whose parent is renamed and recreated',
  'watch skips excluded entries and reports a rename out of them',
  'watch keeps recursive coverage where a non-recursive target overlaps',
  'watch follows a directory replaced at the same path',
  'argv exec passes arguments to the program without shell parsing',
  'exec reports the stream of every chunk in both forms',
  'argv exec honors cwd and exit codes',
  'argv exec reports missing programs and empty argv as spawn errors',
  'windowed exec keeps the exact tail and counts what it skips',
  'argv exec distinguishes timeout from abort',
  'listFiles skips what git ignores in work trees, .git, and excluded names',
]);
registerEnvConformance({ describe, expect, it: (name, run, timeout) => xfails.has(name) ? it.fails(name,run,timeout) : it(name,run,timeout) }, 'HostExecutionEnv WIT conformance', async test => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(),'companion-env-'));
  const env = new HostExecutionEnv(fakeHost(dir),dir);
  try { await test(env); } finally { await env.cleanup(BACKGROUND_CONTEXT); fs.rmSync(dir,{recursive:true,force:true}); }
}, {symlinks:false,listFiles:true});
it('exec is shell_unavailable, never calls output', async () => {
  const env = new HostExecutionEnv(fakeHost('/tmp'),'/doc'); let output = false;
  for (const command of ['echo hello',['echo','hello']]) {
    const r = await env.exec(command,{onOutput:() => {output = true;}},BACKGROUND_CONTEXT);
    expect(r.ok).toBe(false); if (r.ok === false) expect(r.error.code).toBe('shell_unavailable');
  }
  expect(output).toBe(false);
});
