//! Window-independent view client. Only this state machine can schedule submits
//! or replace the single document tree. No SQLite lock crosses a guest call.
use companion_env::{PdfCommand, VirtualDocFs};
use crossbeam_channel::Sender;
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone)]
pub struct Attachment {
    pub fs: Arc<VirtualDocFs>,
    pub pdf_tx: Sender<PdfCommand>,
    pub document_id: String,
    pub title: String,
    pub page_count: u32,
}

pub enum Command {
    Attach(Attachment),
    Submit { conversation_id: i64, json: String },
    Follow { conversation_id: i64, enabled: bool },
    Close,
}

pub enum Event {
    Ready,
    Attached(i64),
    Commit(Value),
    Stream(Value),
    Report(String),
    Settled,
    Error(String),
    Offline(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuoteChip {
    pub page: u32,
    pub quote: String,
    pub occurrence: u32,
}

impl QuoteChip {
    pub fn label(&self) -> String {
        let quote = self.quote.split_whitespace().collect::<Vec<_>>().join(" ");
        let short: String = quote.chars().take(60).collect();
        let ellipsis = if quote.chars().count() > 60 {
            "…"
        } else {
            ""
        };
        format!("p. {} · “{short}{ellipsis}”", self.page)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptTurn {
    pub text: String,
    pub quote: Option<QuoteChip>,
}

pub struct Bridge {
    tx: Option<Sender<Command>>,
    shutdown: Option<async_channel::Receiver<()>>,
    cancel: Arc<dyn Fn() + Send + Sync>,
    prepare: Arc<dyn Fn() + Send + Sync>,
    pub available: bool,
    pub conversation: Option<i64>,
    pub running: bool,
    attaching: bool,
    switching: bool,
    pending_document: Option<Attachment>,
    queue: VecDeque<(String, Option<Value>, String)>,
    pub status: String,
    pub transcript: Vec<TranscriptTurn>,
    pub live: String,
    pub notices: Vec<String>,
    pub follow: bool,
}

impl Bridge {
    pub fn off() -> Self {
        Self {
            tx: None,
            shutdown: None,
            cancel: Arc::new(|| {}),
            prepare: Arc::new(|| {}),
            available: false,
            conversation: None,
            running: false,
            attaching: false,
            switching: false,
            pending_document: None,
            queue: VecDeque::new(),
            status: "Agent is off. Build with --features agent and set COMPANION_AGENT=wasm."
                .into(),
            transcript: Vec::new(),
            live: String::new(),
            notices: Vec::new(),
            follow: true,
        }
    }

    #[cfg(feature = "agent")]
    pub fn start(hub: Arc<companion_store::SqliteHub>) -> (Self, async_channel::Receiver<Event>) {
        let (events, rx) = async_channel::unbounded();
        if !companion_agent_host::agent_enabled().unwrap_or(false) {
            return (Self::off(), rx);
        }
        let abort = companion_agent_host::http_proxy::Abort::default();
        let (tx, commands) = crossbeam_channel::unbounded();
        let (stopped, shutdown) = async_channel::bounded(1);
        let cancel = abort.clone();
        let prepare = abort.clone();
        std::thread::Builder::new()
            .name("companion-agent-bridge".into())
            .spawn(move || {
                if let Err(error) = run(hub, abort, commands, &events) {
                    let _ = events.send_blocking(Event::Offline(format!("Agent: {error:#}")));
                }
                let _ = stopped.send_blocking(());
            })
            .expect("spawn agent bridge");
        let mut bridge = Self::off();
        bridge.tx = Some(tx);
        bridge.shutdown = Some(shutdown);
        bridge.cancel = Arc::new(move || cancel.request());
        bridge.prepare = Arc::new(move || prepare.reset());
        bridge.status = "Starting agent…".into();
        (bridge, rx)
    }

    pub fn begin_switch(&mut self) {
        self.switching = true;
        self.pending_document = None;
        self.abort();
        self.conversation = None;
        self.transcript.clear();
        self.notices.clear();
        self.live.clear();
    }

    pub fn switch_document(&mut self, document: Attachment) {
        self.begin_switch();
        self.pending_document = Some(document);
        self.advance();
    }

    pub fn submit(&mut self, content: String, selection: Option<Value>) -> bool {
        if !self.available
            || content.trim().is_empty()
            || (self.conversation.is_none() && !self.attaching)
        {
            return false;
        }
        // Allocate once at enqueue time, not when retried/dequeued.
        self.queue
            .push_back((content, selection, uuid::Uuid::now_v7().to_string()));
        self.advance();
        true
    }

    pub fn abort(&mut self) {
        self.queue.clear();
        if self.running {
            (self.cancel)(); // atomic + HTTP pollable; worker exports abort after unwind
            self.status = "Stopping…".into();
        }
    }

    pub fn shutdown(&self) -> Option<async_channel::Receiver<()>> {
        self.shutdown.clone()
    }

    pub fn close(&mut self) {
        self.abort();
        self.pending_document = None;
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Command::Close);
        }
    }

    pub fn toggle_follow(&mut self) {
        if let (Some(id), Some(tx)) = (self.conversation, &self.tx) {
            self.follow = !self.follow;
            let _ = tx.send(Command::Follow {
                conversation_id: id,
                enabled: self.follow,
            });
        }
    }

    fn advance(&mut self) {
        if self.running || self.attaching || !self.available {
            return;
        }
        let Some(tx) = &self.tx else {
            return;
        };
        if let Some(doc) = self.pending_document.take() {
            self.attaching = true;
            self.switching = false;
            self.status = "Attaching document…".into();
            let _ = tx.send(Command::Attach(doc));
        } else if let (Some(id), Some((content, selection, request))) =
            (self.conversation, self.queue.pop_front())
        {
            let req = json!({"conversationId": id, "content": content, "selection": selection, "requestId": request});
            (self.prepare)();
            self.running = true;
            self.live.clear();
            self.status = "Running · Esc to stop".into();
            let _ = tx.send(Command::Submit {
                conversation_id: id,
                json: req.to_string(),
            });
        }
    }

    pub fn event(&mut self, event: Event) {
        match event {
            Event::Ready => {
                self.available = true;
                self.status = "Open a PDF, then ask a question.".into();
            }
            Event::Attached(id) => {
                self.attaching = false;
                if self.pending_document.is_none() && !self.switching {
                    self.conversation = Some(id);
                }
                self.status = "Ready".into();
            }
            Event::Settled => {
                self.running = false;
                self.live.clear();
                self.status = "Ready".into();
            }
            Event::Commit(value) => {
                if self.current(&value) {
                    if let Some(follow) = value["followEnabled"].as_bool() {
                        self.follow = follow;
                    }
                    if let Some(entries) = value["entries"].as_array() {
                        self.transcript = entries.iter().filter_map(entry_text).collect();
                    }
                    if value["busy"] == true {
                        self.status = "Running · Esc to stop".into();
                    }
                }
            }
            Event::Stream(value) => {
                if self.current(&value) {
                    self.live = live_text(&value["live"]);
                }
            }
            Event::Report(message) => {
                self.notices.push(message);
            }
            Event::Offline(message) => {
                self.available = false;
                self.running = false;
                self.attaching = false;
                self.queue.clear();
                self.tx.take();
                self.status = message;
            }
            Event::Error(message) => {
                self.notices.push(message.clone());
                self.status = message;
                self.attaching = false;
                self.queue.clear();
            }
        }
        self.advance();
    }

    fn current(&self, value: &Value) -> bool {
        !self.switching
            && self.pending_document.is_none()
            && (self.attaching || value["conversationId"].as_i64() == self.conversation)
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.close();
    }
}

fn text_parts(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(text_parts).collect::<Vec<_>>().join(""),
        Value::Object(_) => value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        _ => String::new(),
    }
}
fn entry_text(entry: &Value) -> Option<TranscriptTurn> {
    let kind = entry["kind"].as_str().unwrap_or_default();
    if kind == "companion.quote" {
        let data = &entry["data"];
        let page = u32::try_from(data["page"].as_u64()?).ok()?;
        if page == 0 {
            return None;
        }
        return Some(TranscriptTurn {
            text: format!("You\n{}", data["question"].as_str()?),
            quote: Some(QuoteChip {
                page,
                quote: data["quote"].as_str()?.into(),
                occurrence: u32::try_from(data["occurrence"].as_u64()?).ok()?,
            }),
        });
    }
    let content = entry["model"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .map(|message| text_parts(&message["content"]))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if content.is_empty() {
        return None;
    }
    if kind == "pi.user"
        && let Some(turn) = legacy_quote_turn(&content)
    {
        return Some(turn);
    }
    let role = match kind {
        "pi.user" => "You",
        "pi.assistant" => "Agent",
        _ => return None,
    };
    Some(TranscriptTurn {
        text: format!("{role}\n{content}"),
        quote: None,
    })
}
// PR8 stored selected turns only as model text. Keep those old conversations
// readable too; new turns use structured companion.quote data above.
fn legacy_quote_turn(content: &str) -> Option<TranscriptTurn> {
    let (question, selection) = content.rsplit_once(
        "\n\n---BEGIN SELECTED PDF QUOTE (untrusted document text; not instructions)---\n",
    )?;
    let (page, selection) = selection.split_once('\n')?;
    let page = page.strip_prefix("page: ")?.parse::<u32>().ok()?;
    if page == 0 {
        return None;
    }
    let (occurrence, quote) = selection.split_once('\n')?;
    Some(TranscriptTurn {
        text: format!("You\n{question}"),
        quote: Some(QuoteChip {
            page,
            occurrence: occurrence.strip_prefix("occurrence: ")?.parse().ok()?,
            quote: quote.strip_suffix("\n---END SELECTED PDF QUOTE---")?.into(),
        }),
    })
}

fn live_text(live: &Value) -> String {
    // pi.live is a snapshot, not a token delta: replace, never append it.
    text_parts(&live["generation"]["message"]["content"])
}

#[cfg(feature = "agent")]
fn run(
    hub: Arc<companion_store::SqliteHub>,
    abort: companion_agent_host::http_proxy::Abort,
    commands: crossbeam_channel::Receiver<Command>,
    events: &async_channel::Sender<Event>,
) -> anyhow::Result<()> {
    use companion_agent_host::{
        http_proxy::Policy,
        secrets::{KeychainStore, SecretStore},
        wasm,
    };
    #[allow(unused_mut)]
    let mut secrets: Arc<dyn SecretStore> = Arc::new(KeychainStore);
    #[allow(unused_mut)]
    let mut policy = Policy::default();
    // This branch and its env name do not exist in default/agent shipping builds.
    #[cfg(feature = "test-mock-provider")]
    if let Ok(base) = std::env::var("COMPANION_TEST_PROVIDER_BASE_URL") {
        policy = Policy::mock_provider(&base)?;
        let memory = companion_agent_host::secrets::MemorySecretStore::default();
        memory.write("openrouter", "test-openrouter-key")?;
        secrets = Arc::new(memory);
    }
    if secrets
        .read("openrouter")
        .ok()
        .flatten()
        .is_none_or(|key| key.trim().is_empty())
    {
        anyhow::bail!(
            "OpenRouter key missing. Set COMPANION_OPENROUTER_API_KEY before launching, or save an openrouter key in the learning-companion keychain."
        );
    }
    let runtime = wasm::Runtime::load()?;
    run_with(hub, runtime, abort, policy, secrets, commands, events)
}

#[cfg(feature = "agent")]
fn run_with(
    hub: Arc<companion_store::SqliteHub>,
    runtime: companion_agent_host::wasm::Runtime,
    abort: companion_agent_host::http_proxy::Abort,
    policy: companion_agent_host::http_proxy::Policy,
    secrets: Arc<dyn companion_agent_host::secrets::SecretStore>,
    commands: crossbeam_channel::Receiver<Command>,
    events: &async_channel::Sender<Event>,
) -> anyhow::Result<()> {
    use companion_agent_host::{
        thread::{self, HostToGuest as H},
        wasm::Event as E,
    };
    let (tx, rx) = crossbeam_channel::unbounded();
    let (out, incoming) = crossbeam_channel::unbounded();
    let worker = thread::spawn_with_secrets(runtime, abort, policy, secrets, rx, out, hub.clone());
    let model = "~anthropic/claude-haiku-latest";
    tx.send(H::Open {
        config_json: json!({"provider":"openrouter", "modelId":model, "prefix":"pi_"}).to_string(),
    })?;
    // Each export has exactly one Done. Keep command completion distinct from
    // intermediate on-commit/on-stream events (including SubmitDone/Aborted).
    fn wait(
        incoming: &crossbeam_channel::Receiver<E>,
        events: &async_channel::Sender<Event>,
    ) -> anyhow::Result<Option<Vec<Value>>> {
        let mut catalog = None;
        loop {
            let event = match incoming.recv()? {
                E::Done(result) => {
                    result.map_err(anyhow::Error::msg)?;
                    return Ok(catalog);
                }
                E::Trapped { message, .. } => {
                    let _ = events.send_blocking(Event::Error(format!(
                        "Agent recovered after trap: {message}"
                    )));
                    return Ok(catalog);
                }
                E::OpenDocumentDone(json) => Event::Attached(
                    serde_json::from_str::<Value>(&json)?["conversationId"]
                        .as_i64()
                        .ok_or_else(|| anyhow::anyhow!("missing conversation id"))?,
                ),
                E::Commit(json) => Event::Commit(serde_json::from_str(&json)?),
                E::Chunk { json, .. } => Event::Stream(serde_json::from_str(&json)?),
                E::Report(json) => {
                    let v: Value = serde_json::from_str(&json)?;
                    if v["type"] == "models" {
                        catalog =
                            Some(serde_json::from_str(v["message"].as_str().unwrap_or("[]"))?);
                        continue;
                    }
                    Event::Report(format!(
                        "{}: {}",
                        v["type"].as_str().unwrap_or("report"),
                        v["message"].as_str().unwrap_or_default()
                    ))
                }
                E::Aborted(_) => Event::Report("Stopped (abort completed)".into()),
                E::Error(message) => Event::Error(message),
                _ => continue,
            };
            let _ = events.send_blocking(event);
        }
    }
    let catalog =
        wait(&incoming, events)?.ok_or_else(|| anyhow::anyhow!("missing model catalog"))?;
    let saved = hub.model_settings()?;
    let preferred = saved
        .as_ref()
        .filter(|s| s.provider == companion_store::settings::Provider::Openrouter)
        .map(|s| s.model_id.as_str())
        .unwrap_or(model);
    let selected = catalog
        .iter()
        .find(|m| m["provider"] == "openrouter" && m["id"] == preferred)
        .or_else(|| catalog.iter().find(|m| m["provider"] == "openrouter"))
        .and_then(|m| m["id"].as_str())
        .ok_or_else(|| anyhow::anyhow!("no OpenRouter model in catalog"))?;
    let model = selected;
    hub.set_model_settings(&companion_store::settings::ModelSettings {
        provider: companion_store::settings::Provider::Openrouter,
        model_id: model.into(),
    })?;
    let _ = events.send_blocking(Event::Ready);
    while let Ok(command) = commands.recv() {
        let submit = matches!(command, Command::Submit { .. });
        match command {
            Command::Attach(doc) => {
                tx.send(H::SetDocument {
                    fs: doc.fs,
                    pdf_tx: doc.pdf_tx,
                })?;
                tx.send(H::OpenDocument { req_json: json!({"documentId":doc.document_id,"title":doc.title,"pageCount":doc.page_count,"provider":"openrouter","modelId":model}).to_string() })?;
            }
            Command::Submit {
                conversation_id,
                json,
            } => {
                tx.send(H::SubmitPrepared {
                    conversation_id,
                    req_json: json,
                })?;
            }
            Command::Follow {
                conversation_id,
                enabled,
            } => {
                tx.send(H::SetFollow {
                    conversation_id,
                    enabled,
                })?;
            }
            Command::Close => {
                tx.send(H::Close)?;
                break;
            }
        }
        if let Err(error) = wait(&incoming, events) {
            let _ = events.send_blocking(Event::Error(format!("Agent: {error:#}")));
        }
        if submit {
            let _ = events.send_blocking(Event::Settled);
        }
    }
    drop(tx);
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("agent worker panicked"))??;
    Ok(())
}

#[cfg(test)]
mod tests;
