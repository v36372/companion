#[cfg_attr(not(feature = "agent"), allow(dead_code))]
mod agent_bridge;
mod app;
mod ask_bar;
mod chat;
mod model_picker;
mod selection_bar;
mod window;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "companion=info,gpui_pdf=warn".into()),
        )
        .init();

    let mut args = std::env::args_os().skip(1);
    let path = args.next().map(std::path::PathBuf::from);
    anyhow::ensure!(
        args.next().is_none(),
        "usage: learning-companion [path.pdf]"
    );
    app::run(path);
    Ok(())
}
