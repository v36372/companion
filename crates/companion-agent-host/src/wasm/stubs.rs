// PR4 capability stubs: no filesystem, SQLite or PDF access.
use super::{AgentHostState, companion::agent::types::*};
impl super::companion::agent::fs::Host for AgentHostState {
    fn read_text_file(&mut self, _path: String) -> Result<String, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn read_binary_file(&mut self, _path: String) -> Result<Vec<u8>, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn write_file(&mut self, _path: String, _content: Vec<u8>) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn append_file(&mut self, _path: String, _content: Vec<u8>) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn truncate_file(&mut self, _path: String, _size: u64) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn flush_file(&mut self, _path: String) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn rename_file(&mut self, _from: String, _to: String) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn file_info(&mut self, _path: String) -> Result<FileInfo, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn list_dir(&mut self, _path: String) -> Result<Vec<FileInfo>, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn list_files(&mut self, _path: String, _opts: ListFilesOptions) -> Result<String, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn exists(&mut self, _path: String) -> Result<bool, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn create_dir(&mut self, _path: String, _recursive: bool) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn remove(&mut self, _path: String, _recursive: bool, _force: bool) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn canonical_path(&mut self, _path: String) -> Result<String, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn absolute_path(&mut self, _cwd: String, _path: String) -> Result<String, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn join_path(&mut self, _parts: Vec<String>) -> Result<String, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn create_temp_dir(&mut self, _prefix: Option<String>) -> Result<String, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn create_temp_file(
        &mut self,
        _prefix: Option<String>,
        _suffix: Option<String>,
    ) -> Result<String, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn open_binary(&mut self, _path: String, _no_follow: bool) -> Result<u32, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn pread(&mut self, _handle: u32, _offset: u64, _length: u32) -> Result<Vec<u8>, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn scan_lines(
        &mut self,
        _handle: u32,
        _start_line: u32,
        _end_line: Option<u32>,
    ) -> Result<LineScan, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn fstat(&mut self, _handle: u32) -> Result<FileInfo, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn close_handle(&mut self, _handle: u32) -> () {}
    fn read_text_lines(
        &mut self,
        _path: String,
        _max_lines: Option<u32>,
    ) -> Result<Vec<String>, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn open_text_line_reader(&mut self, _path: String) -> Result<u32, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn read_line(&mut self, _handle: u32) -> Result<Option<TextLine>, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn watch_register(&mut self, _targets_json: String) -> Result<u32, FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn watch_close(&mut self, _id: u32) -> () {}
    fn env_id(&mut self) -> String {
        "PR4 stub".into()
    }
    fn env_cwd(&mut self) -> String {
        "PR4 stub".into()
    }
    fn set_cwd(&mut self, _path: String) -> Result<(), FsError> {
        Err(FsError {
            code: "not_supported".into(),
            message: "PR4 stub".into(),
            path: None,
        })
    }
    fn fs_cleanup(&mut self) -> () {}
    fn exec(&mut self, _command_json: String, _options_json: String) -> Result<String, ExecError> {
        Err(ExecError {
            code: "shell_unavailable".into(),
            message: "no shell".into(),
        })
    }
}
impl super::companion::agent::sqlite::Host for AgentHostState {
    fn exec(&mut self, _sql: String) -> Result<(), SqliteError> {
        Err(SqliteError {
            message: "PR4 stub".into(),
        })
    }
    fn run(&mut self, _sql: String, _params: Vec<SqliteValue>) -> Result<(), SqliteError> {
        Err(SqliteError {
            message: "PR4 stub".into(),
        })
    }
    fn get(
        &mut self,
        _sql: String,
        _params: Vec<SqliteValue>,
    ) -> Result<Option<String>, SqliteError> {
        Err(SqliteError {
            message: "PR4 stub".into(),
        })
    }
    fn all(&mut self, _sql: String, _params: Vec<SqliteValue>) -> Result<String, SqliteError> {
        Err(SqliteError {
            message: "PR4 stub".into(),
        })
    }
    fn begin(&mut self) -> Result<(), SqliteError> {
        Err(SqliteError {
            message: "PR4 stub".into(),
        })
    }
    fn commit(&mut self) -> Result<(), SqliteError> {
        Err(SqliteError {
            message: "PR4 stub".into(),
        })
    }
    fn rollback(&mut self) -> Result<(), SqliteError> {
        Err(SqliteError {
            message: "PR4 stub".into(),
        })
    }
}
impl super::companion::agent::pdf_cmd::Host for AgentHostState {
    fn goto(&mut self, _page: u32, _quote: Option<String>, _occurrence: u32) -> Result<(), String> {
        Err("PR4 stub".into())
    }
    fn highlight(
        &mut self,
        _page: u32,
        _quote: String,
        _occurrence: u32,
        _color: Option<String>,
    ) -> Result<u64, String> {
        Err("PR4 stub".into())
    }
    fn search(&mut self, _query: String, _max: u32) -> Result<String, String> {
        Err("PR4 stub".into())
    }
}
