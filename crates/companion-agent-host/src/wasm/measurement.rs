//! Run in separate processes so every sample includes component loading.
use super::*;

#[test]
#[ignore = "manual component load + instantiate timing (no provider/credentials)"]
fn component_time_to_ready() {
    measure(false);
}

#[test]
#[ignore = "manual baseline: original main compilation path, no cache"]
fn component_time_to_ready_before() {
    measure(true);
}

fn measure(before: bool) {
    let start = Instant::now();
    let runtime = if before {
        // Exact pre-cache Runtime::load implementation from main e5e8239.
        let engine = engine().unwrap();
        let compile_start = Instant::now();
        let component =
            Component::from_binary(&engine, include_bytes!("../agent-core.wasm")).unwrap();
        Runtime {
            engine,
            component,
            compile_ms: compile_start.elapsed().as_secs_f64() * 1000.,
        }
    } else {
        Runtime::load().unwrap()
    };
    let loaded = start.elapsed();
    let (events, _) = crossbeam_channel::unbounded();
    let state = AgentHostState::with_secrets(
        Abort::default(),
        Policy::default(),
        events,
        std::sync::Arc::new(crate::secrets::MemorySecretStore::default()),
        std::sync::Arc::new(companion_store::SqliteHub::open(":memory:").unwrap()),
    );
    let _instance = instantiate(&runtime, state).unwrap();
    println!(
        "component ready: load_ms={:.1} instantiate_ms={:.1} total_ms={:.1}",
        loaded.as_secs_f64() * 1000.,
        (start.elapsed() - loaded).as_secs_f64() * 1000.,
        start.elapsed().as_secs_f64() * 1000.
    );
}
