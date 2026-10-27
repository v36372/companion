//! One Store per plain OS thread. Commands block on recv; exports stay on-stack.
use crate::{
    http_proxy::{Abort, Policy},
    wasm::{self, AgentHostState, Event},
};
use crossbeam_channel::{Receiver, Sender};

pub enum Command {
    Sse(String),
    Busy,
    Close,
}

pub fn spawn(
    runtime: wasm::Runtime,
    abort: Abort,
    policy: Policy,
    commands: Receiver<Command>,
    events: Sender<Event>,
) -> std::thread::JoinHandle<anyhow::Result<()>> {
    std::thread::Builder::new()
        .name("companion-agent".into())
        .spawn(move || {
            let state = AgentHostState::new(abort, policy, events.clone());
            let (mut store, guest) = wasm::instantiate(&runtime, state)?;
            while let Ok(command) = commands.recv() {
                let result = match command {
                    Command::Sse(url) => guest
                        .call_spike_sse(&mut store, &url)
                        .map_err(|e| format!("{e:#}")),
                    Command::Busy => guest
                        .call_spike_busy(&mut store)
                        .map(|_| Ok(String::new()))
                        .map_err(|e| format!("{e:#}")),
                    Command::Close => break,
                };
                let trapped = result.is_err();
                let _ = events.send(Event::Done(result.and_then(|r| r)));
                // Never reuse a trapped instance. PR5 must reinstantiate+open before
                // abort or resume-wait; this spike exits and drops the Store instead.
                if trapped {
                    break;
                }
            }
            Ok(())
        })
        .expect("spawn agent thread")
}

#[cfg(test)]
mod tests;
