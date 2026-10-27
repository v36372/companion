declare module 'companion:agent/sqlite@0.1.0' {
  export function exec(sql: string): void;
  export function run(sql: string, params: import('./storage.ts').WireValue[]): void;
  export function get(sql: string, params: import('./storage.ts').WireValue[]): string | undefined;
  export function all(sql: string, params: import('./storage.ts').WireValue[]): string;
  export function begin(): void;
  export function commit(): void;
  export function rollback(): void;
}
declare module 'companion:agent/host-events@0.1.0' {
  export function onCommit(json: string): void;
  export function onStream(json: string): void;
  export function onReport(json: string): void;
}

declare module 'companion:agent/fs@0.1.0' {
  export const envId: import('./env.ts').FsHost['envId'];
  export const absolutePath: import('./env.ts').FsHost['absolutePath'];
  export const joinPath: import('./env.ts').FsHost['joinPath'];
  export const readTextFile: import('./env.ts').FsHost['readTextFile'];
  export const readBinaryFile: import('./env.ts').FsHost['readBinaryFile'];
  export const readTextLines: import('./env.ts').FsHost['readTextLines'];
  export const openTextLineReader: import('./env.ts').FsHost['openTextLineReader'];
  export const readLine: import('./env.ts').FsHost['readLine'];
  export const openBinary: import('./env.ts').FsHost['openBinary'];
  export const pread: import('./env.ts').FsHost['pread'];
  export const scanLines: import('./env.ts').FsHost['scanLines'];
  export const fstat: import('./env.ts').FsHost['fstat'];
  export const closeHandle: import('./env.ts').FsHost['closeHandle'];
  export const writeFile: import('./env.ts').FsHost['writeFile'];
  export const appendFile: import('./env.ts').FsHost['appendFile'];
  export const truncateFile: import('./env.ts').FsHost['truncateFile'];
  export const flushFile: import('./env.ts').FsHost['flushFile'];
  export const renameFile: import('./env.ts').FsHost['renameFile'];
  export const fileInfo: import('./env.ts').FsHost['fileInfo'];
  export const listDir: import('./env.ts').FsHost['listDir'];
  export const listFiles: import('./env.ts').FsHost['listFiles'];
  export const canonicalPath: import('./env.ts').FsHost['canonicalPath'];
  export const exists: import('./env.ts').FsHost['exists'];
  export const createDir: import('./env.ts').FsHost['createDir'];
  export const remove: import('./env.ts').FsHost['remove'];
  export const createTempDir: import('./env.ts').FsHost['createTempDir'];
  export const createTempFile: import('./env.ts').FsHost['createTempFile'];
  export const watchRegister: import('./env.ts').FsHost['watchRegister'];
  export const watchClose: import('./env.ts').FsHost['watchClose'];
  export const fsCleanup: import('./env.ts').FsHost['fsCleanup'];
  export const exec: import('./env.ts').FsHost['exec'];
}
declare module 'companion:agent/pdf-cmd@0.1.0' {
  export const goto: import('./extensions/pdf.ts').PdfHost['goto'];
  export const highlight: import('./extensions/pdf.ts').PdfHost['highlight'];
  export const search: import('./extensions/pdf.ts').PdfHost['search'];
}
