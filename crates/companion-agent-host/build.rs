fn main() {
    println!("cargo:rerun-if-changed=src/agent-core.wasm");
    if std::env::var_os("CARGO_FEATURE_AGENT").is_some()
        && !std::path::Path::new("src/agent-core.wasm").is_file()
    {
        panic!(
            "agent-core.wasm is missing: cd agent && npm ci && npm run build && npm run componentize (or build without --features agent)"
        );
    }
}
