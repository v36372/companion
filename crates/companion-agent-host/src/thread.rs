//! One Store per plain OS thread. Commands block on recv; exports stay on-stack.
use crate::{
    http_proxy::{Abort, Policy},
    secrets::{KeychainStore, SecretStore},
    wasm::{self, AgentHostState, Event},
};
use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;

pub enum HostToGuest {
    Open {
        config_json: String,
    },
    OpenDocument {
        req_json: String,
    },
    Submit {
        req_json: String,
        conversation_id: i64,
    },
    Abort {
        conversation_id: i64,
    },
    ResumeWait,
    SetFollow {
        conversation_id: i64,
        enabled: bool,
    },
    Configure {
        conversation_id: i64,
        agent_json: String,
    },
    // PR4 regression checks; not a generation pump.
    Sse(String),
    Busy,
    Close,
}
pub type Command = HostToGuest;
pub type GuestToHost = Event;

pub fn spawn(
    runtime: wasm::Runtime,
    abort: Abort,
    policy: Policy,
    commands: Receiver<Command>,
    events: Sender<Event>,
) -> std::thread::JoinHandle<anyhow::Result<()>> {
    spawn_with_secrets(
        runtime,
        abort,
        policy,
        Arc::new(KeychainStore),
        commands,
        events,
    )
}

pub fn spawn_with_secrets(
    runtime: wasm::Runtime,
    abort: Abort,
    policy: Policy,
    secrets: Arc<dyn SecretStore>,
    commands: Receiver<HostToGuest>,
    events: Sender<GuestToHost>,
) -> std::thread::JoinHandle<anyhow::Result<()>> {
    std::thread::Builder::new()
        .name("companion-agent".into())
        .spawn(move || {
            let state = || {
                AgentHostState::with_secrets(
                    abort.clone(),
                    policy.clone(),
                    events.clone(),
                    secrets.clone(),
                )
            };
            let (mut store, mut guest) = wasm::instantiate(&runtime, state())?;
            let mut last_open = None;
            while let Ok(command) = commands.recv() {
                let mut conversation_id = None;
                let mut submit = false;
                let result = match command {
                    Command::Open { config_json } => {
                        last_open = Some(config_json.clone());
                        guest
                            .call_open(&mut store, &config_json)
                            .map(|r| r.map(|_| String::new()))
                    }
                    Command::OpenDocument { req_json } => {
                        guest.call_open_document(&mut store, &req_json).map(|r| {
                            r.inspect(|json| {
                                let _ = events.send(Event::OpenDocumentDone(json.clone()));
                            })
                        })
                    }
                    Command::Submit {
                        req_json,
                        conversation_id: id,
                    } => {
                        conversation_id = Some(id);
                        submit = true;
                        abort.reset();
                        guest.call_submit(&mut store, &req_json)
                    }
                    Command::Abort {
                        conversation_id: id,
                    } => guest.call_abort(&mut store, id).map(|r| {
                        r.map(|_| {
                            let _ = events.send(Event::Aborted(id));
                            String::new()
                        })
                    }),
                    Command::ResumeWait => guest
                        .call_resume_wait(&mut store)
                        .map(|r| r.map(|_| String::new())),
                    Command::SetFollow {
                        conversation_id,
                        enabled,
                    } => guest
                        .call_set_follow(&mut store, conversation_id, enabled)
                        .map(|r| r.map(|_| String::new())),
                    Command::Configure {
                        conversation_id,
                        agent_json,
                    } => guest
                        .call_configure(&mut store, conversation_id, &agent_json)
                        .map(|r| r.map(|_| String::new())),
                    Command::Sse(url) => guest.call_spike_sse(&mut store, &url),
                    Command::Busy => guest.call_spike_busy(&mut store).map(|_| Ok(String::new())),
                    Command::Close => {
                        guest.call_close(&mut store)?;
                        break;
                    }
                };
                match result {
                    Ok(result) => {
                        // A rejected fetch is a failed run, not a Durable abort.
                        // Only call the abort export once submit is off-stack.
                        if submit && abort.requested() {
                            let id = conversation_id.unwrap();
                            guest
                                .call_abort(&mut store, id)?
                                .map_err(anyhow::Error::msg)?;
                            let _ = events.send(Event::Aborted(id));
                        }
                        if submit {
                            match &result {
                                Ok(json) => {
                                    let _ = events.send(Event::SubmitDone(json.clone()));
                                }
                                Err(error) => {
                                    let _ = events.send(Event::Error(error.clone()));
                                }
                            }
                        }
                        let _ = events.send(Event::Done(result));
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        let abort_requested = abort.requested();
                        // Drop the trapped Store/resource table before any export.
                        // Guest has only non-owning typed handles (no Drop); replace
                        // them below and never invoke a handle from the old Store.
                        drop(store);
                        let Some(config) = &last_open else {
                            let _ = events.send(Event::Done(Err(message)));
                            break;
                        };
                        (store, guest) = wasm::instantiate(&runtime, state())?;
                        guest
                            .call_open(&mut store, config)?
                            .map_err(anyhow::Error::msg)?;
                        if abort_requested {
                            if let Some(id) = conversation_id {
                                match guest.call_abort(&mut store, id)? {
                                    Ok(()) => {
                                        let _ = events.send(Event::Aborted(id));
                                    }
                                    Err(error) => {
                                        let _ = events.send(Event::Error(error));
                                    }
                                }
                            }
                        } else {
                            guest
                                .call_resume_wait(&mut store)?
                                .map_err(anyhow::Error::msg)?;
                        }
                        let _ = events.send(Event::Trapped {
                            message,
                            abort_requested,
                        });
                    }
                }
            }
            Ok(())
        })
        .expect("spawn agent thread")
}

#[cfg(test)]
mod tests;
