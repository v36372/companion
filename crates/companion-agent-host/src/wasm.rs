use crate::http_proxy::{Abort, Policy};
use crossbeam_channel::Sender;
use std::time::Instant;
use wasmtime::{
    Engine, Store,
    component::{Component, Linker, ResourceTable},
};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::WasiHttpCtx;

// Wasmtime 36.0.19 replaced the old `async: false` option with per-function
// flags. Explicitly discard WIT async defaults: ALL imports/exports are sync.
wasmtime::component::bindgen!({
    path: "../../wit", world: "guest",
    imports: { default: ignore_wit },
    exports: { default: ignore_wit },
});

#[derive(Debug)]
pub enum Event {
    Chunk {
        at: Instant,
        json: String,
    },
    Ready {
        cold_start_ms: f64,
        compile_ms: f64,
    },
    Done(Result<String, String>),
    Commit(String),
    Report(String),
    OpenDocumentDone(String),
    SubmitDone(String),
    Aborted(i64),
    Error(String),
    Trapped {
        message: String,
        abort_requested: bool,
    },
}

pub struct AgentHostState {
    pub fs: std::sync::Arc<companion_env::VirtualDocFs>,
    pub pdf_tx: Sender<companion_env::PdfCommand>,
    pub sqlite: std::sync::Arc<companion_store::SqliteHub>,
    sqlite_owner: u64,
    pub secrets: std::sync::Arc<dyn crate::secrets::SecretStore>,
    pub abort: Abort,
    pub policy: Policy,
    pub events: Sender<Event>,
    pub table: ResourceTable,
    pub wasi: WasiCtx,
    pub http: WasiHttpCtx,
}
impl AgentHostState {
    pub fn new(
        abort: Abort,
        policy: Policy,
        events: Sender<Event>,
        sqlite: std::sync::Arc<companion_store::SqliteHub>,
    ) -> Self {
        Self::with_secrets(
            abort,
            policy,
            events,
            std::sync::Arc::new(crate::secrets::KeychainStore),
            sqlite,
        )
    }
    pub fn with_secrets(
        abort: Abort,
        policy: Policy,
        events: Sender<Event>,
        secrets: std::sync::Arc<dyn crate::secrets::SecretStore>,
        sqlite: std::sync::Arc<companion_store::SqliteHub>,
    ) -> Self {
        let mut wasi = WasiCtxBuilder::new();
        // Diagnostics only, never inherited environment or filesystem access.
        #[cfg(test)]
        wasi.inherit_stderr();
        static NEXT_OWNER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            fs: std::sync::Arc::new(
                companion_env::VirtualDocFs::new(None, sqlite.clone())
                    .expect("empty document environment"),
            ),
            pdf_tx: crossbeam_channel::unbounded().0,
            sqlite,
            sqlite_owner: NEXT_OWNER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            secrets,
            abort,
            policy,
            events,
            table: ResourceTable::new(),
            // EMPTY environment/arguments; no inherited stdio or preopens.
            wasi: wasi.build(),
            http: WasiHttpCtx::new(),
        }
    }
}
impl Drop for AgentHostState {
    fn drop(&mut self) {
        self.sqlite.release(self.sqlite_owner);
    }
}
impl WasiView for AgentHostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

pub fn engine() -> anyhow::Result<Engine> {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true).epoch_interruption(true);
    // This spike prioritizes reproducible cold compilation over throughput.
    config.cranelift_opt_level(wasmtime::OptLevel::None);
    // Sync embedder only: no async_support, runtime, or block_on.
    Engine::new(&config)
}
/// Compile once; reuse the immutable component, never a Store or trapped instance.
#[derive(Clone)]
pub struct Runtime {
    pub engine: Engine,
    component: Component,
    compile_ms: f64,
}
impl Runtime {
    pub fn load() -> anyhow::Result<Self> {
        anyhow::ensure!(crate::agent_enabled()?, "agent is disabled");
        let engine = engine()?;
        let start = Instant::now();
        let component = Component::from_binary(&engine, include_bytes!("agent-core.wasm"))?;
        Ok(Self {
            engine,
            component,
            compile_ms: start.elapsed().as_secs_f64() * 1000.,
        })
    }
}
pub fn instantiate(
    runtime: &Runtime,
    state: AgentHostState,
) -> anyhow::Result<(Store<AgentHostState>, Guest)> {
    let engine = &runtime.engine;
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    wasmtime_wasi_http::add_only_http_to_linker_sync(&mut linker)?;
    Guest::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)?;
    let mut store = Store::new(engine, state);
    store.set_epoch_deadline(1);
    store.epoch_deadline_trap();
    let start = Instant::now();
    let guest = Guest::instantiate(&mut store, &runtime.component, &linker)?;
    let _ = store.data().events.send(Event::Ready {
        cold_start_ms: start.elapsed().as_secs_f64() * 1000.,
        compile_ms: runtime.compile_ms,
    });
    Ok((store, guest))
}

impl companion::agent::types::Host for AgentHostState {}
impl companion::agent::host_events::Host for AgentHostState {
    fn websearch_enabled(&mut self) -> bool {
        self.sqlite.websearch_enabled().unwrap_or(false)
            && self
                .secrets
                .read("brave")
                .ok()
                .flatten()
                .is_some_and(|key| !key.trim().is_empty())
    }
    fn on_commit(&mut self, json: String) {
        let _ = self.events.send(Event::Commit(json));
    }
    fn on_report(&mut self, json: String) {
        let _ = self.events.send(Event::Report(json));
    }
    fn on_stream(&mut self, json: String) {
        let _ = self.events.send(Event::Chunk {
            at: Instant::now(),
            json,
        });
    }
}

mod env;
mod sqlite;
#[cfg(test)]
mod sqlite_tests;
