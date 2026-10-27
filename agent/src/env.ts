import type { Context } from '@earendil-works/chord';
import {
  ok, err, FileError, ExecutionError,
  type ExecutionEnv, type Result, type FileInfo, type BinaryReader, type TextLineReader,
  type DirReader, type FileWatcher, type WatchChange, type WatchTarget,
  type ListFilesOptions, type FileListing, type LineScan, type TextLine,
  type ShellExecOptions, type ShellExecResult,
} from '@earendil-works/pi-durable/env';

export interface FsHost {
  envId(): string;
  absolutePath(cwd: string, path: string): string;
  joinPath(parts: string[]): string;
  readTextFile(path: string): string;
  readBinaryFile(path: string): Uint8Array;
  readTextLines(path: string, maxLines?: number): string[];
  openTextLineReader(path: string): number;
  readLine(handle: number): TextLine | undefined;
  openBinary(path: string, noFollow: boolean): number;
  pread(handle: number, offset: bigint, length: number): Uint8Array;
  scanLines(handle: number, start: number, end?: number): { [K in keyof LineScan]: number | bigint };
  fstat(handle: number): Omit<FileInfo, 'size'> & { size: number | bigint };
  closeHandle(handle: number): void;
  writeFile(path: string, content: Uint8Array): void;
  appendFile(path: string, content: Uint8Array): void;
  truncateFile(path: string, size: bigint): void;
  flushFile(path: string): void;
  renameFile(from: string, to: string): void;
  fileInfo(path: string): Omit<FileInfo, 'size'> & { size: number | bigint };
  listDir(path: string): (Omit<FileInfo, 'size'> & { size: number | bigint })[];
  listFiles(path: string, opts: { hidden?: boolean; gitignore?: boolean; exclude: string[]; maxEntries?: number; maxBytes?: number; timeoutMs?: number }): string;
  canonicalPath(path: string): string;
  exists(path: string): boolean;
  createDir(path: string, recursive: boolean): void;
  remove(path: string, recursive: boolean, force: boolean): void;
  createTempDir(prefix?: string): string;
  createTempFile(prefix?: string, suffix?: string): string;
  watchRegister(targets: string): number;
  watchClose(handle: number): void;
  fsCleanup(): void;
  exec(command: string, options: string): string;
}
// ComponentizeJS lowers WIT result<T,E> to return T / throw E (not tagged JS results).
function fileError(e: unknown): FileError {
  const value = e as { payload?: unknown; code?: string; message?: string; path?: string };
  if (value?.payload) return fileError(value.payload);
  return new FileError((value?.code ?? 'unknown') as FileError['code'], value?.message ?? String(e), value?.path);
}
function call<T>(fn: () => T, context?: Context): Result<T, FileError> { if (context?.abortSignal?.aborted) return err(new FileError('aborted', 'operation aborted')); try { return ok(fn()); } catch(e) { return err(fileError(e)); } }
function info(i: ReturnType<FsHost['fileInfo']>): FileInfo { return { ...i, size: Number(i.size) }; }
const bytes = (value: string | Uint8Array) => typeof value === 'string' ? new TextEncoder().encode(value) : value;

export class HostExecutionEnv implements ExecutionEnv {
  readonly id: string;
  cwd: string;
  private readonly host: FsHost;
  constructor(host: FsHost, cwd = '/doc') { this.host = host; this.id = host.envId(); this.cwd = cwd; }
  private path(path: string) { return this.host.absolutePath(this.cwd, path); }
  async absolutePath(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.path(path)); }
  async joinPath(parts: string[], _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.joinPath(parts)); }
  async readTextFile(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.readTextFile(this.path(path))); }
  async readBinaryFile(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.readBinaryFile(this.path(path))); }
  async readTextLines(path: string, opts: { maxLines?: number } | undefined, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.readTextLines(this.path(path), opts?.maxLines)); }
  async openTextLineReader(path: string, _ctx: Context): Promise<Result<TextLineReader, FileError>> {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted'));
    return call(() => {
      const handle = this.host.openTextLineReader(this.path(path));
      return {
        readLine: async (_c: Context) => call(() => this.host.readLine(handle), _c),
        close: async (_c: Context) => { this.host.closeHandle(handle); },
      };
    });
  }
  async openBinaryReader(path: string, opts: { noFollow?: boolean } | undefined, _ctx: Context): Promise<Result<BinaryReader, FileError>> {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted'));
    return call(() => {
      const handle = this.host.openBinary(this.path(path), opts?.noFollow ?? false);
      return {
        info: async (_c: Context) => call(() => info(this.host.fstat(handle)), _c),
        read: async (offset: number, length: number, _c: Context) => call(() => { if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(length) || length < 0) throw new FileError('invalid', 'invalid read range'); return this.host.pread(handle, BigInt(offset), length); }, _c),
        scanLines: async (opts: { startLine: number; endLine?: number }, _c: Context) => call(() => {
          if (_c.abortSignal?.aborted) throw new FileError('aborted', 'operation aborted');
          if (!Number.isSafeInteger(opts.startLine) || opts.startLine < 0 || (opts.endLine !== undefined && (!Number.isSafeInteger(opts.endLine) || opts.endLine <= opts.startLine))) throw new FileError('invalid', 'invalid line range');
          const scan = this.host.scanLines(handle, opts.startLine, opts.endLine);
          return Object.fromEntries(Object.entries(scan).map(([key, value]) => [key, Number(value)])) as unknown as LineScan;
        }),
        close: async (_c: Context) => { this.host.closeHandle(handle); },
      };
    });
  }
  async writeFile(path: string, value: string | Uint8Array, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.writeFile(this.path(path), bytes(value))); }
  async appendFile(path: string, value: string | Uint8Array, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.appendFile(this.path(path), bytes(value))); }
  async truncateFile(path: string, size: number, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.truncateFile(this.path(path), BigInt(size))); }
  async flushFile(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.flushFile(this.path(path))); }
  async renameFile(from: string, to: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.renameFile(this.path(from), this.path(to))); }
  async fileInfo(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => info(this.host.fileInfo(this.path(path)))); }
  async listDir(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.listDir(this.path(path)).map(info)); }
  async listFiles(path: string, opts: ListFilesOptions | undefined, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => {
      for (const value of [opts?.maxEntries, opts?.maxBytes, opts?.timeoutMs]) {
        if (value !== undefined && (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff)) throw new FileError('invalid', 'invalid listing bound');
      }
      return JSON.parse(this.host.listFiles(this.path(path), { ...opts, exclude: [...(opts?.exclude ?? [])] })) as FileListing;
    }); }
  async openDirReader(path: string, ctx: Context): Promise<Result<DirReader, FileError>> {
    const listed = await this.listDir(path, ctx); if (listed.ok === false) return err(listed.error);
    let offset = 0; let closed = false;
    return ok({ next: async (max: number, _c: Context) => {
      if (_c.abortSignal?.aborted) return err(new FileError('aborted', 'operation aborted'));
      if (closed || !Number.isSafeInteger(max) || max <= 0) return err(new FileError('invalid', 'closed reader or invalid page size'));
      const entries: FileInfo[] = [];
      while (offset < listed.value.length && entries.length < max) {
        const entry = listed.value[offset++];
        const current = call(() => info(this.host.fileInfo(entry.path)));
        if (current.ok) entries.push(current.value);
      }
      return ok({ entries, done: offset >= listed.value.length });
    }, close: async (_c: Context) => { closed = true; } });
  }
  async watch(targets: readonly WatchTarget[], _onChange: (change: WatchChange) => void, _ctx: Context): Promise<Result<FileWatcher, FileError>> {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted'));
    return call(() => { const handle = this.host.watchRegister(JSON.stringify(targets)); let closed = false;
      return { mode: 'polling', close: async (_c: Context) => { if (!closed) { this.host.watchClose(handle); closed = true; } } };
    });
  }
  async canonicalPath(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.canonicalPath(this.path(path))); }
  async exists(path: string, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.exists(this.path(path))); }
  async createDir(path: string, opts: { recursive?: boolean } | undefined, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.createDir(this.path(path), opts?.recursive ?? false)); }
  async remove(path: string, opts: { recursive?: boolean; force?: boolean } | undefined, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.remove(this.path(path), opts?.recursive ?? false, opts?.force ?? false)); }
  async createTempDir(prefix: string | undefined, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.createTempDir(prefix)); }
  async createTempFile(opts: { prefix?: string; suffix?: string } | undefined, _ctx: Context) {
    if (_ctx.abortSignal?.aborted) return err<never, FileError>(new FileError('aborted', 'operation aborted')); return call(() => this.host.createTempFile(opts?.prefix, opts?.suffix)); }
  async cleanup(_ctx: Context) { this.host.fsCleanup(); }
  async exec(command: string | readonly string[], opts: ShellExecOptions | undefined, _ctx: Context): Promise<Result<ShellExecResult, ExecutionError>> {
    if (_ctx.abortSignal?.aborted) return err(new ExecutionError('aborted', 'operation aborted'));
    try { return ok(JSON.parse(this.host.exec(JSON.stringify(command), JSON.stringify(opts ?? {}))) as ShellExecResult); }
    catch(e) { const value = fileError(e); return err(new ExecutionError(value.code as ExecutionError['code'], value.message)); }
  }
}
