//! Thin WIT adapters; capability policy lives in companion-env.
use super::{AgentHostState, companion::agent::types::*};
impl From<companion_env::FsError> for FsError {
    fn from(e: companion_env::FsError) -> Self {
        Self {
            code: e.code,
            message: e.message,
            path: e.path,
        }
    }
}
impl From<companion_env::FileInfo> for FileInfo {
    fn from(v: companion_env::FileInfo) -> Self {
        Self {
            name: v.name,
            path: v.path,
            kind: v.kind,
            size: v.size,
            mtime_ms: v.mtime_ms,
        }
    }
}
impl From<companion_env::LineScan> for LineScan {
    fn from(v: companion_env::LineScan) -> Self {
        Self {
            newlines: v.newlines,
            start: v.start,
            end: v.end,
            first_line_end: v.first_line_end,
            last_line_start: v.last_line_start,
            selected_bytes: v.selected_bytes,
            first_line_bytes: v.first_line_bytes,
        }
    }
}
impl From<companion_env::TextLine> for TextLine {
    fn from(v: companion_env::TextLine) -> Self {
        Self {
            text: v.text,
            terminated: v.terminated,
        }
    }
}
impl super::companion::agent::fs::Host for AgentHostState {
    fn read_text_file(&mut self, path: String) -> Result<String, FsError> {
        self.fs.read_text_file(&path).map_err(Into::into)
    }
    fn read_binary_file(&mut self, path: String) -> Result<Vec<u8>, FsError> {
        self.fs.read_binary_file(&path).map_err(Into::into)
    }
    fn write_file(&mut self, path: String, content: Vec<u8>) -> Result<(), FsError> {
        self.fs.write_file(&path, content).map_err(Into::into)
    }
    fn append_file(&mut self, path: String, content: Vec<u8>) -> Result<(), FsError> {
        self.fs.append_file(&path, content).map_err(Into::into)
    }
    fn truncate_file(&mut self, path: String, size: u64) -> Result<(), FsError> {
        self.fs.truncate_file(&path, size).map_err(Into::into)
    }
    fn flush_file(&mut self, path: String) -> Result<(), FsError> {
        self.fs.flush_file(&path).map_err(Into::into)
    }
    fn rename_file(&mut self, from: String, to: String) -> Result<(), FsError> {
        self.fs.rename_file(&from, &to).map_err(Into::into)
    }
    fn file_info(&mut self, path: String) -> Result<FileInfo, FsError> {
        self.fs.file_info(&path).map(Into::into).map_err(Into::into)
    }
    fn list_dir(&mut self, path: String) -> Result<Vec<FileInfo>, FsError> {
        self.fs
            .list_dir(&path)
            .map(|items| items.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }
    fn list_files(&mut self, path: String, opts: ListFilesOptions) -> Result<String, FsError> {
        self.fs
            .list_files(
                &path,
                companion_env::ListFilesOptions {
                    hidden: opts.hidden,
                    gitignore: opts.gitignore,
                    exclude: opts.exclude,
                    max_entries: opts.max_entries,
                    max_bytes: opts.max_bytes,
                    timeout_ms: opts.timeout_ms,
                },
            )
            .map_err(Into::into)
    }
    fn exists(&mut self, path: String) -> Result<bool, FsError> {
        self.fs.exists(&path).map_err(Into::into)
    }
    fn create_dir(&mut self, path: String, recursive: bool) -> Result<(), FsError> {
        self.fs.create_dir(&path, recursive).map_err(Into::into)
    }
    fn remove(&mut self, path: String, recursive: bool, force: bool) -> Result<(), FsError> {
        self.fs.remove(&path, recursive, force).map_err(Into::into)
    }
    fn canonical_path(&mut self, path: String) -> Result<String, FsError> {
        self.fs.canonical_path(&path).map_err(Into::into)
    }
    fn absolute_path(&mut self, cwd: String, path: String) -> Result<String, FsError> {
        self.fs.absolute_path(&cwd, &path).map_err(Into::into)
    }
    fn join_path(&mut self, parts: Vec<String>) -> Result<String, FsError> {
        self.fs.join_path(parts).map_err(Into::into)
    }
    fn create_temp_dir(&mut self, prefix: Option<String>) -> Result<String, FsError> {
        self.fs.create_temp_dir(prefix).map_err(Into::into)
    }
    fn create_temp_file(
        &mut self,
        prefix: Option<String>,
        suffix: Option<String>,
    ) -> Result<String, FsError> {
        self.fs.create_temp_file(prefix, suffix).map_err(Into::into)
    }
    fn open_binary(&mut self, path: String, no_follow: bool) -> Result<u32, FsError> {
        self.fs.open_binary(&path, no_follow).map_err(Into::into)
    }
    fn pread(&mut self, handle: u32, offset: u64, length: u32) -> Result<Vec<u8>, FsError> {
        self.fs.pread(handle, offset, length).map_err(Into::into)
    }
    fn scan_lines(
        &mut self,
        handle: u32,
        start_line: u32,
        end_line: Option<u32>,
    ) -> Result<LineScan, FsError> {
        self.fs
            .scan_lines(handle, start_line, end_line)
            .map(Into::into)
            .map_err(Into::into)
    }
    fn fstat(&mut self, handle: u32) -> Result<FileInfo, FsError> {
        self.fs.fstat(handle).map(Into::into).map_err(Into::into)
    }
    fn close_handle(&mut self, handle: u32) {
        self.fs.close_handle(handle)
    }
    fn read_text_lines(
        &mut self,
        path: String,
        max_lines: Option<u32>,
    ) -> Result<Vec<String>, FsError> {
        self.fs
            .read_text_lines(&path, max_lines)
            .map_err(Into::into)
    }
    fn open_text_line_reader(&mut self, path: String) -> Result<u32, FsError> {
        self.fs.open_text_line_reader(&path).map_err(Into::into)
    }
    fn read_line(&mut self, handle: u32) -> Result<Option<TextLine>, FsError> {
        self.fs
            .read_line(handle)
            .map(|v| v.map(Into::into))
            .map_err(Into::into)
    }
    fn watch_register(&mut self, targets_json: String) -> Result<u32, FsError> {
        self.fs.watch_register(&targets_json).map_err(Into::into)
    }
    fn watch_close(&mut self, id: u32) {
        self.fs.watch_close(id)
    }
    fn env_id(&mut self) -> String {
        self.fs.env_id()
    }
    fn env_cwd(&mut self) -> String {
        self.fs.env_cwd()
    }
    fn set_cwd(&mut self, path: String) -> Result<(), FsError> {
        self.fs.set_cwd(&path).map_err(Into::into)
    }
    fn fs_cleanup(&mut self) {
        self.fs.fs_cleanup()
    }
    fn exec(&mut self, command_json: String, options_json: String) -> Result<String, ExecError> {
        self.fs
            .exec(&command_json, &options_json)
            .map_err(|e| ExecError {
                code: e.code,
                message: e.message,
            })
    }
}
impl AgentHostState {
    fn pdf_commands(&self) -> companion_env::PdfCommands {
        companion_env::PdfCommands {
            fs: self.fs.clone(),
            hub: self.sqlite.clone(),
            tx: self.pdf_tx.clone(),
        }
    }
}
impl super::companion::agent::pdf_cmd::Host for AgentHostState {
    fn goto(&mut self, page: u32, quote: Option<String>, occurrence: u32) -> Result<(), String> {
        self.pdf_commands().goto(page, quote, occurrence)
    }
    fn highlight(
        &mut self,
        page: u32,
        quote: String,
        occurrence: u32,
        color: Option<String>,
    ) -> Result<u64, String> {
        self.pdf_commands()
            .highlight(page, quote, occurrence, color)
    }
    fn search(&mut self, query: String, max: u32) -> Result<String, String> {
        self.pdf_commands().search(&query, max)
    }
}
