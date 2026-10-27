use companion_pdf::snapshot::DocSnapshot;
use companion_store::SqliteHub;
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub type FsResult<T> = Result<T, FsError>;
#[derive(Clone, Debug, PartialEq)]
pub struct ExecError {
    pub code: String,
    pub message: String,
}
const TEXT_CAP: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub struct FsError {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}
impl FsError {
    pub fn new(code: &str, message: impl Into<String>, path: Option<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            path,
        }
    }
}
fn error(code: &str, path: &str) -> FsError {
    FsError::new(code, code, Some(path.into()))
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub mtime_ms: f64,
}
#[derive(Clone, Debug, Default)]
pub struct ListFilesOptions {
    pub hidden: Option<bool>,
    pub gitignore: Option<bool>,
    pub exclude: Vec<String>,
    pub max_entries: Option<u32>,
    pub max_bytes: Option<u32>,
    pub timeout_ms: Option<u32>,
}
#[derive(Clone, Debug, Default)]
pub struct LineScan {
    pub newlines: u32,
    pub start: u64,
    pub end: u64,
    pub first_line_end: u64,
    pub last_line_start: u64,
    pub selected_bytes: u64,
    pub first_line_bytes: u64,
}
#[derive(Clone, Debug)]
pub struct TextLine {
    pub text: String,
    pub terminated: bool,
}
struct Handle {
    bytes: Vec<u8>,
    info: FileInfo,
    offset: usize,
}
struct State {
    cwd: String,
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
    handles: HashMap<u32, Handle>,
    next: u32,
}
pub struct VirtualDocFs {
    pub snapshot: Option<Arc<DocSnapshot>>,
    hub: Arc<SqliteHub>,
    state: Mutex<State>,
    pub follow: AtomicBool,
}

/// POSIX normalization, never URL interpretation or host path resolution.
pub fn absolute_path(cwd: &str, path: &str) -> FsResult<String> {
    if path.contains('\0')
        || path.contains("file://")
        || cwd.contains('\0')
        || cwd.contains("file://")
    {
        return Err(error("invalid", path));
    }
    let joined = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{cwd}/{path}")
    };
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => (),
            ".." => {
                if parts.pop().is_none() {
                    return Err(error("invalid", path));
                }
            }
            p => parts.push(p),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(p, _)| if p.is_empty() { "/" } else { p })
        .unwrap_or("/")
}
fn writable(path: &str) -> FsResult<()> {
    if path.starts_with("/notes/") || path.starts_with("/tmp/") {
        Ok(())
    } else {
        Err(error("permission_denied", path))
    }
}
impl VirtualDocFs {
    /// No snapshot is useful before the first document opens; reads fail closed.
    pub fn new(snapshot: Option<Arc<DocSnapshot>>, hub: Arc<SqliteHub>) -> FsResult<Self> {
        let files = if let Some(doc) = &snapshot {
            crate::notes::load(&hub, &doc.meta.id)?
        } else {
            BTreeMap::new()
        };
        let mut dirs: BTreeSet<String> = ["/", "/doc", "/doc/pages", "/notes", "/tmp"]
            .into_iter()
            .map(String::from)
            .collect();
        for path in files.keys() {
            let mut p = parent(path);
            while p != "/" {
                dirs.insert(p.into());
                p = parent(p);
            }
        }
        Ok(Self {
            snapshot,
            hub,
            state: Mutex::new(State {
                cwd: "/doc".into(),
                files,
                dirs,
                handles: HashMap::new(),
                next: 1,
            }),
            follow: AtomicBool::new(true),
        })
    }
    pub fn env_id(&self) -> String {
        format!(
            "companion-doc:{}",
            self.snapshot.as_ref().map_or("none", |d| &d.meta.id)
        )
    }
    pub fn env_cwd(&self) -> String {
        self.state.lock().cwd.clone()
    }
    pub fn absolute_path(&self, cwd: &str, path: &str) -> FsResult<String> {
        absolute_path(cwd, path)
    }
    pub fn join_path(&self, parts: Vec<String>) -> FsResult<String> {
        let joined = parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        let normalized = absolute_path("/", &joined)?;
        if joined.starts_with('/') {
            Ok(normalized)
        } else {
            let relative = normalized.strip_prefix('/').unwrap();
            Ok(if relative.is_empty() { "." } else { relative }.to_string())
        }
    }
    fn path(&self, path: &str) -> FsResult<String> {
        absolute_path(&self.state.lock().cwd, path)
    }
    fn doc_files(&self) -> Vec<String> {
        let mut paths: Vec<String> = [
            "meta.json",
            "outline.md",
            "viewport.json",
            "selection.json",
            "follow.json",
        ]
        .iter()
        .map(|p| format!("/doc/{p}"))
        .collect();
        if let Some(doc) = &self.snapshot {
            for p in 1..=doc.meta.page_count {
                for ext in ["txt", "json"] {
                    paths.push(format!("/doc/pages/{p:04}.{ext}"));
                }
            }
        }
        paths
    }
    fn doc_read(&self, path: &str) -> FsResult<Vec<u8>> {
        let doc = self
            .snapshot
            .as_ref()
            .ok_or_else(|| error("not_found", path))?;
        let bytes = match path {
            "/doc/meta.json" => serde_json::to_vec(&doc.meta),
            "/doc/outline.md" => return Ok(doc.outline_md.as_bytes().to_vec()),
            "/doc/viewport.json" => serde_json::to_vec(&*doc.viewport.read().unwrap()),
            "/doc/selection.json" => serde_json::to_vec(&*doc.selection.read().unwrap()),
            "/doc/follow.json" => serde_json::to_vec(
                &serde_json::json!({"enabled":self.follow.load(Ordering::Relaxed)}),
            ),
            _ => {
                if !self.doc_files().iter().any(|p| p == path) {
                    return Err(error("not_found", path));
                }
                let name = path
                    .strip_prefix("/doc/pages/")
                    .ok_or_else(|| error("not_found", path))?;
                let (number, ext) = name
                    .split_once('.')
                    .ok_or_else(|| error("not_found", path))?;
                let page: usize = number.parse().map_err(|_| error("not_found", path))?;
                let slot = doc
                    .pages
                    .get(page.wrapping_sub(1))
                    .ok_or_else(|| error("not_found", path))?;
                if ext == "txt" {
                    return slot
                        .text
                        .as_ref()
                        .map(|s| s.as_bytes().to_vec())
                        .ok_or_else(|| {
                            FsError::new("not_supported", "no_text_layer", Some(path.into()))
                        });
                }
                serde_json::to_vec(&*slot.json)
            }
        };
        bytes.map_err(|e| FsError::new("unknown", e.to_string(), Some(path.into())))
    }
    pub fn read_binary_file(&self, path: &str) -> FsResult<Vec<u8>> {
        let path = self.path(path)?;
        if self.state.lock().dirs.contains(&path) {
            return Err(error("is_directory", &path));
        }
        if path.starts_with("/doc/") {
            return self.doc_read(&path);
        }
        self.state
            .lock()
            .files
            .get(&path)
            .cloned()
            .ok_or_else(|| error("not_found", &path))
    }
    pub fn read_text_file(&self, path: &str) -> FsResult<String> {
        let bytes = self.read_binary_file(path)?;
        if bytes.len() > TEXT_CAP {
            return Err(error("invalid", path));
        }
        String::from_utf8(bytes).map_err(|_| error("invalid", path))
    }
    fn mutate<T>(&self, op: impl FnOnce(&mut State) -> FsResult<T>) -> FsResult<T> {
        let mut state = self.state.lock();
        let old_files = state.files.clone();
        let old_dirs = state.dirs.clone();
        let result = op(&mut state).and_then(|value| {
            if let Some(doc) = &self.snapshot {
                crate::notes::save(&self.hub, &doc.meta.id, &state.files)?;
            }
            Ok(value)
        });
        if result.is_err() {
            state.files = old_files;
            state.dirs = old_dirs;
        }
        result
    }
    pub fn write_file(&self, path: &str, bytes: Vec<u8>) -> FsResult<()> {
        self.write(path, bytes, false)
    }
    pub fn append_file(&self, path: &str, bytes: Vec<u8>) -> FsResult<()> {
        self.write(path, bytes, true)
    }
    fn write(&self, path: &str, bytes: Vec<u8>, append: bool) -> FsResult<()> {
        let path = self.path(path)?;
        writable(&path)?;
        self.mutate(|s| {
            if s.dirs.contains(&path) {
                return Err(error("is_directory", &path));
            }
            let mut ancestor = parent(&path);
            while ancestor != "/" {
                if s.files.contains_key(ancestor) {
                    return Err(error("not_directory", ancestor));
                }
                s.dirs.insert(ancestor.into());
                ancestor = parent(ancestor);
            }
            if append {
                s.files.entry(path).or_default().extend(bytes);
            } else {
                s.files.insert(path, bytes);
            }
            Ok(())
        })
    }
    pub fn truncate_file(&self, path: &str, size: u64) -> FsResult<()> {
        let path = self.path(path)?;
        writable(&path)?;
        let size = usize::try_from(size)
            .ok()
            .filter(|n| *n <= TEXT_CAP)
            .ok_or_else(|| error("invalid", &path))?;
        self.mutate(|s| {
            s.files
                .get_mut(&path)
                .ok_or_else(|| error("not_found", &path))?
                .resize(size, 0);
            Ok(())
        })
    }
    pub fn flush_file(&self, path: &str) -> FsResult<()> {
        let p = self.path(path)?;
        writable(&p)?;
        self.file_info(&p).map(|_| ())
    }
    pub fn rename_file(&self, from: &str, to: &str) -> FsResult<()> {
        let from = self.path(from)?;
        let to = self.path(to)?;
        writable(&from)?;
        writable(&to)?;
        if from.split('/').nth(1) != to.split('/').nth(1) {
            return Err(error("invalid", &to));
        }
        self.mutate(|s| {
            if !s.dirs.contains(parent(&to)) {
                return Err(error("not_found", &to));
            }
            if s.dirs.contains(&from) || s.dirs.contains(&to) {
                return Err(error("is_directory", &from));
            }
            let bytes = s
                .files
                .remove(&from)
                .ok_or_else(|| error("not_found", &from))?;
            s.files.insert(to, bytes);
            Ok(())
        })
    }
    pub fn file_info(&self, path: &str) -> FsResult<FileInfo> {
        let path = self.path(path)?;
        let (kind, size) = {
            let s = self.state.lock();
            if s.dirs.contains(&path) {
                ("directory", 0)
            } else if let Some(b) = s.files.get(&path) {
                ("file", b.len() as u64)
            } else {
                drop(s);
                match self.doc_read(&path) {
                    Ok(b) => ("file", b.len() as u64),
                    Err(e) if e.message == "no_text_layer" => ("file", 0),
                    Err(e) => return Err(e),
                }
            }
        };
        Ok(FileInfo {
            name: path.rsplit('/').next().unwrap_or_default().into(),
            path,
            kind: kind.into(),
            size,
            mtime_ms: 0.0,
        })
    }
    pub fn exists(&self, path: &str) -> FsResult<bool> {
        match self.file_info(path) {
            Ok(_) => Ok(true),
            Err(e) if e.code == "not_found" => Ok(false),
            Err(e) => Err(e),
        }
    }
    pub fn canonical_path(&self, path: &str) -> FsResult<String> {
        let p = self.path(path)?;
        self.file_info(&p)?;
        Ok(p)
    }
    pub fn set_cwd(&self, path: &str) -> FsResult<()> {
        let p = self.path(path)?;
        if self.file_info(&p)?.kind != "directory" {
            return Err(error("not_directory", &p));
        }
        self.state.lock().cwd = p;
        Ok(())
    }
    pub fn list_dir(&self, path: &str) -> FsResult<Vec<FileInfo>> {
        let p = self.path(path)?;
        if self.file_info(&p)?.kind != "directory" {
            return Err(error("not_directory", &p));
        }
        let s = self.state.lock();
        let mut paths: BTreeSet<String> = s
            .files
            .keys()
            .chain(s.dirs.iter())
            .filter(|key| key.as_str() != p && parent(key) == p)
            .cloned()
            .collect();
        drop(s);
        paths.extend(self.doc_files().into_iter().filter(|key| parent(key) == p));
        paths.into_iter().map(|key| self.file_info(&key)).collect()
    }
    pub fn list_files(&self, path: &str, opts: ListFilesOptions) -> FsResult<String> {
        let start = Instant::now();
        let root = self.path(path)?;
        let mut pending: Vec<FileInfo> = self.list_dir(&root)?.into_iter().rev().collect();
        let mut entries = Vec::new();
        let mut bytes = 0;
        let mut truncated = false;
        while let Some(entry) = pending.pop() {
            if entry.name == ".git"
                || (opts.hidden == Some(false) && entry.name.starts_with('.'))
                || opts.exclude.contains(&entry.name)
            {
                continue;
            }
            let relative = entry
                .path
                .strip_prefix(&format!("{}/", root.trim_end_matches('/')))
                .unwrap_or(&entry.path)
                .to_string();
            bytes += relative.len() + 2;
            if entries.len() >= opts.max_entries.unwrap_or(50_000).min(50_000) as usize
                || bytes
                    > opts
                        .max_bytes
                        .unwrap_or(4 * 1024 * 1024)
                        .min(4 * 1024 * 1024) as usize
                || start.elapsed().as_millis()
                    >= opts.timeout_ms.unwrap_or(10_000).min(10_000) as u128
            {
                truncated = true;
                break;
            }
            entries.push(serde_json::json!({"path":relative,"kind":entry.kind}));
            if entry.kind == "directory" {
                pending.extend(self.list_dir(&entry.path)?.into_iter().rev());
            }
        }
        Ok(serde_json::json!({"entries":entries,"truncated":truncated}).to_string())
    }
    pub fn create_dir(&self, path: &str, recursive: bool) -> FsResult<()> {
        let p = self.path(path)?;
        writable(&p)?;
        self.mutate(|s| {
            if s.files.contains_key(&p) {
                return Err(error("invalid", &p));
            }
            if !recursive && !s.dirs.contains(parent(&p)) {
                return Err(error("not_found", parent(&p)));
            }
            let mut next = p.as_str();
            while next != "/" {
                if s.files.contains_key(next) {
                    return Err(error("not_directory", next));
                }
                s.dirs.insert(next.into());
                if !recursive {
                    break;
                }
                next = parent(next);
            }
            Ok(())
        })
    }
    pub fn remove(&self, path: &str, recursive: bool, force: bool) -> FsResult<()> {
        let p = self.path(path)?;
        writable(&p)?;
        self.mutate(|s| {
            let prefix = format!("{p}/");
            if s.dirs.contains(&p) {
                if !recursive
                    && (s.files.keys().any(|k| k.starts_with(&prefix))
                        || s.dirs.iter().any(|k| k.starts_with(&prefix)))
                {
                    return Err(error("invalid", &p));
                }
                s.dirs.retain(|k| k != &p && !k.starts_with(&prefix));
                s.files.retain(|k, _| !k.starts_with(&prefix));
            } else if s.files.remove(&p).is_none() && !force {
                return Err(error("not_found", &p));
            }
            Ok(())
        })
    }
    fn temp_path(&self, prefix: Option<String>, suffix: Option<String>) -> FsResult<String> {
        let prefix = prefix.unwrap_or_default();
        let suffix = suffix.unwrap_or_default();
        if prefix.contains('/') || suffix.contains('/') {
            return Err(error("invalid", &prefix));
        }
        self.path(&format!("/tmp/{prefix}{}{suffix}", uuid::Uuid::new_v4()))
    }
    pub fn create_temp_dir(&self, prefix: Option<String>) -> FsResult<String> {
        let p = self.temp_path(prefix, None)?;
        self.create_dir(&p, false)?;
        Ok(p)
    }
    pub fn create_temp_file(
        &self,
        prefix: Option<String>,
        suffix: Option<String>,
    ) -> FsResult<String> {
        let p = self.temp_path(prefix, suffix)?;
        self.write_file(&p, vec![])?;
        Ok(p)
    }
    pub fn open_binary(&self, path: &str, _no_follow: bool) -> FsResult<u32> {
        let bytes = self.read_binary_file(path)?;
        let info = self.file_info(path)?;
        let mut s = self.state.lock();
        let id = s.next;
        s.next += 1;
        s.handles.insert(
            id,
            Handle {
                bytes,
                info,
                offset: 0,
            },
        );
        Ok(id)
    }
    pub fn pread(&self, handle: u32, offset: u64, length: u32) -> FsResult<Vec<u8>> {
        let s = self.state.lock();
        let h = s
            .handles
            .get(&handle)
            .ok_or_else(|| error("invalid", "handle"))?;
        let offset = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(h.bytes.len());
        Ok(h.bytes[offset..offset.saturating_add(length as usize).min(h.bytes.len())].to_vec())
    }
    pub fn fstat(&self, handle: u32) -> FsResult<FileInfo> {
        self.state
            .lock()
            .handles
            .get(&handle)
            .map(|h| h.info.clone())
            .ok_or_else(|| error("invalid", "handle"))
    }
    pub fn close_handle(&self, handle: u32) {
        self.state.lock().handles.remove(&handle);
    }
    pub fn open_text_line_reader(&self, path: &str) -> FsResult<u32> {
        self.open_binary(path, false)
    }
    pub fn read_line(&self, handle: u32) -> FsResult<Option<TextLine>> {
        let mut s = self.state.lock();
        let h = s
            .handles
            .get_mut(&handle)
            .ok_or_else(|| error("invalid", "handle"))?;
        if h.offset == 0 && h.bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            h.offset = 3;
        }
        if h.offset >= h.bytes.len() {
            return Ok(None);
        }
        let start = h.offset;
        let end = h.bytes[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| start + n);
        let terminated = end.is_some();
        let end = end.unwrap_or(h.bytes.len());
        h.offset = end + usize::from(terminated);
        let content_end = end;
        Ok(Some(TextLine {
            text: String::from_utf8_lossy(&h.bytes[start..content_end]).into_owned(),
            terminated,
        }))
    }
    pub fn read_text_lines(&self, path: &str, max_lines: Option<u32>) -> FsResult<Vec<String>> {
        let handle = self.open_text_line_reader(path)?;
        let result = (|| {
            let mut lines = Vec::new();
            while lines.len() < max_lines.unwrap_or(u32::MAX) as usize {
                match self.read_line(handle)? {
                    Some(line) => lines.push(line.text),
                    None => break,
                }
            }
            Ok(lines)
        })();
        self.close_handle(handle);
        result
    }
    pub fn scan_lines(
        &self,
        handle: u32,
        start_line: u32,
        end_line: Option<u32>,
    ) -> FsResult<LineScan> {
        if end_line.is_some_and(|end| end <= start_line) {
            return Err(error("invalid", "line range"));
        }
        let s = self.state.lock();
        let h = s
            .handles
            .get(&handle)
            .ok_or_else(|| error("invalid", "handle"))?;
        let mut starts = vec![0];
        starts.extend(
            h.bytes
                .iter()
                .enumerate()
                .filter(|(_, b)| **b == b'\n')
                .map(|(i, _)| i + 1),
        );
        let size = h.bytes.len();
        let newlines = (starts.len() - 1) as u32;
        let Some(&start) = starts.get(start_line as usize) else {
            return Ok(LineScan {
                newlines,
                start: size as u64,
                end: size as u64,
                first_line_end: size as u64,
                last_line_start: size as u64,
                ..Default::default()
            });
        };
        let last = end_line.map_or(starts.len() - 1, |end| {
            (end as usize - 1).min(starts.len() - 1)
        });
        let end = starts.get(last + 1).map_or(size, |n| n - 1);
        let first_end = starts.get(start_line as usize + 1).map_or(size, |n| n - 1);
        let decoded = |from: usize, to: usize| {
            let from = if from == 0 && h.bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
                3.min(to)
            } else {
                from
            };
            String::from_utf8_lossy(&h.bytes[from..to]).len() as u64
        };
        Ok(LineScan {
            newlines,
            start: start as u64,
            end: end as u64,
            first_line_end: first_end as u64,
            last_line_start: starts[last] as u64,
            selected_bytes: decoded(start, end),
            first_line_bytes: decoded(start, first_end),
        })
    }
    pub fn watch_register(&self, _targets: &str) -> FsResult<u32> {
        let mut s = self.state.lock();
        let id = s.next;
        s.next += 1;
        Ok(id)
    }
    pub fn watch_close(&self, _id: u32) {}
    pub fn exec(&self, _command_json: &str, _options_json: &str) -> Result<String, ExecError> {
        Err(ExecError {
            code: "shell_unavailable".into(),
            message: "no shell in learning-companion v1".into(),
        })
    }
    pub fn fs_cleanup(&self) {
        let mut s = self.state.lock();
        s.handles.clear();
        s.files.retain(|p, _| !p.starts_with("/tmp/"));
        s.dirs.retain(|p| !p.starts_with("/tmp/"));
    }
}
