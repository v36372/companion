import type { SqliteDatabase, SqliteExecutor, SqliteValue } from '@earendil-works/chord/sqlite';

export type WireValue = { tag: 'null' } | { tag: 'real'; val: number } |
  { tag: 'integer'; val: bigint } | { tag: 'text'; val: string } | { tag: 'blob'; val: Uint8Array };
export interface SqliteHost {
  exec(sql: string): void;
  run(sql: string, params: WireValue[]): void;
  get(sql: string, params: WireValue[]): string | undefined;
  all(sql: string, params: WireValue[]): string;
  begin(): void;
  commit(): void;
  rollback(): void;
}
const never = <T>(): Promise<T> => new Promise(() => {});
export function encode(value: SqliteValue): WireValue {
  if (value === null) return { tag: 'null' };
  if (typeof value === 'bigint') return { tag: 'integer', val: value };
  if (typeof value === 'number') return Number.isSafeInteger(value)
    ? { tag: 'integer', val: BigInt(value) } : { tag: 'real', val: value };
  if (typeof value === 'string') return { tag: 'text', val: value };
  return { tag: 'blob', val: value };
}
export function decodeRow<T extends object>(json: string): T {
  const row = JSON.parse(json);
  for (const key of Object.keys(row)) {
    const value = row[key];
    if (value.tag === 'null') row[key] = null;
    else if (value.tag === 'blob') row[key] = new Uint8Array(value.val);
    else if (value.tag === 'integer') {
      const integer = BigInt(value.val);
      row[key] = integer >= BigInt(Number.MIN_SAFE_INTEGER) && integer <= BigInt(Number.MAX_SAFE_INTEGER)
        ? Number(integer) : integer;
    } else row[key] = value.val;
  }
  return row as T;
}
class HostExecutor implements SqliteExecutor {
  active = true;
  protected readonly host: SqliteHost;
  private readonly isTx: boolean;
  private readonly blocked: () => boolean;
  constructor(host: SqliteHost, isTx: boolean, blocked: () => boolean) {
    this.host = host;
    this.isTx = isTx;
    this.blocked = blocked;
  }
  private check(): boolean {
    if (!this.active) throw new Error('transaction handle is no longer active');
    return !this.isTx && this.blocked();
  }
  async exec(sql: string): Promise<void> {
    if (this.check()) return never();
    this.host.exec(sql); // WIT result errors are thrown by ComponentizeJS.
  }
  async run(sql: string, ...params: SqliteValue[]): Promise<void> {
    if (this.check()) return never();
    this.host.run(sql, params.map(encode));
  }
  async get<T extends object>(sql: string, ...params: SqliteValue[]): Promise<T | undefined> {
    if (this.check()) return never();
    const row = this.host.get(sql, params.map(encode));
    return row === undefined ? undefined : decodeRow<T>(row);
  }
  async all<T extends object>(sql: string, ...params: SqliteValue[]): Promise<T[]> {
    if (this.check()) return never();
    return (JSON.parse(this.host.all(sql, params.map(encode))) as object[])
      .map(row => decodeRow<T>(JSON.stringify(row)));
  }
}
export class HostSqliteDatabase extends HostExecutor implements SqliteDatabase {
  inCallback = false;
  constructor(host: SqliteHost) {
    super(host, false, () => this.inCallback);
  }
  async transaction<T>(callback: (tx: SqliteExecutor) => Promise<T>): Promise<T> {
    if (this.inCallback) return never();
    this.host.begin();
    this.inCallback = true;
    const tx = new HostExecutor(this.host, true, () => false);
    try {
      const result = await callback(tx);
      this.host.commit();
      return result;
    } catch (error) {
      try { this.host.rollback(); }
      catch (rollback) { throw new AggregateError([error, rollback], 'rollback failed'); }
      throw error;
    } finally {
      tx.active = false;
      this.inCallback = false;
    }
  }
  async close(): Promise<void> {
    if (this.inCallback) return never();
  }
}
