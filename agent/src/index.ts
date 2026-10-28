// agent/src/index.ts
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import { createModels } from "@earendil-works/pi-ai/models";
import { openaiProvider } from "@earendil-works/pi-ai/providers/openai";
import { anthropicProvider } from "@earendil-works/pi-ai/providers/anthropic";
import { openrouterProvider } from "@earendil-works/pi-ai/providers/openrouter";
import {
  createRegistry,
  Harness,
  type Conversation,
  type ConversationId,
  type TaskId,
  type AgentChange as DurableAgentChange,
} from "@earendil-works/pi-durable";

import { HostExecutionEnv } from './env.ts';
import { createPdfExtension } from './extensions/pdf.ts';
import { createSnapshotReadTool } from './extensions/read.ts';
import { OpenDoc, FollowMode, ReadingExtension } from './extensions/reading.ts';
import { userInput } from './user-content.ts';
import { createWebsearchExtension } from './extensions/websearch.ts';
import * as fs from 'companion:agent/fs@0.1.0';
import * as pdfCmd from 'companion:agent/pdf-cmd@0.1.0';
import { SqliteStorage } from '@earendil-works/pi-durable/storage/sqlite';
import * as sqlite from 'companion:agent/sqlite@0.1.0';
import { HostSqliteDatabase } from './storage.ts';
import { dummyCredentials } from "./credentials.ts";
import type {
  OpenConfig, OpenDocumentReq, OpenDocumentRes, SubmitReq, SubmitRes, CommitEvent, StreamEvent,
} from "./protocol.ts";
import * as hostEvents from 'companion:agent/host-events@0.1.0';
export { spikeSse, spikeBusy } from './spike.ts';
import './streams.ts';

let harness: Harness | undefined;
let database: HostSqliteDatabase;
let creatingDocument: OpenDocumentReq | undefined;
let attached: { conv: Conversation; dispose: () => void; followChanged: (enabled: boolean) => void } | undefined;

function installProvider(models: ReturnType<typeof createModels>, provider: OpenConfig["provider"]) {
  // v1 default is openrouter. openai/anthropic remain available if settings switch.
  if (provider === "openrouter") models.setProvider(openrouterProvider());
  else if (provider === "openai") models.setProvider(openaiProvider());
  else if (provider === "anthropic") models.setProvider(anthropicProvider());
}

export async function open(configJson: string): Promise<void> {
  if (harness) await close();
  const config = JSON.parse(configJson) as OpenConfig;
  if (!config.provider) config.provider = "openrouter";
  const models = createModels({ credentials: dummyCredentials,
    authContext: { env: async () => undefined, fileExists: async () => false },
  });
  installProvider(models, config.provider);
  // Host overwrite of Authorization is the only strip. No guest transformHeaders
  // (HarnessSettings.stream has none; Models.stream() is called inside the Harness).

  const registry = createRegistry();
  registry.install(createPdfExtension(pdfCmd));
  registry.install(ReadingExtension);
  const websearch = createWebsearchExtension(hostEvents.websearchEnabled());
  if (websearch) registry.install(websearch);
  registry.install({ name: 'read', tools: [createSnapshotReadTool()] });
  database = new HostSqliteDatabase(sqlite);
  const storage = await SqliteStorage.open(database, { prefix: 'pi_' });
  hostEvents.onReport(JSON.stringify({ type: 'models', message: JSON.stringify(models.getModels()) }));

  harness = await Harness.open(
    storage,
    {
      models,
      registry,
      env: (target, _ctx) => new HostExecutionEnv(fs, target.cwd ?? '/doc'),
      conversationCreated: async (tx, conversation) => {
        const doc = await tx.doc(OpenDoc, conversation.id);
        await tx.doc(FollowMode, conversation.id);
        if (creatingDocument) {
          Object.assign(doc, { documentId: creatingDocument.documentId,
            title: creatingDocument.title, pageCount: creatingDocument.pageCount });
        }
      },
      identity: {
        attribution: { name: "learning-companion", url: "https://learning-companion.local" },
      },
      // Failed/aborted stream must not start another model attempt (Esc ≠ retry).
      // 1.4.0 ConversationRetryPolicy: { enabled, maxRetries, baseDelayMs, maxAgentDelayMs? }
      // There is no maxAttempts.
      settings: {
        retry: { enabled: false, maxRetries: 0 },
        afterFailedRun: "withdraw",
      },
      onReport: (error, info) => {
        hostEvents.onReport(JSON.stringify({
          type: info.type,
          message: error instanceof Error ? error.message : String(error),
          task: 'task' in info ? info.task : undefined,
        }));
      },
    },
    BACKGROUND_CONTEXT,
  );
}

export async function openDocument(reqJson: string): Promise<string> {
  if (!harness) throw new Error("harness closed");
  const req = JSON.parse(reqJson) as OpenDocumentReq;
  if (!await database.get('SELECT id FROM documents WHERE id = ?', req.documentId))
    throw new Error('unknown document');
  const mapped = await database.get<{ conversation_id: number }>(
    'SELECT conversation_id FROM conversations_map WHERE document_id = ?', req.documentId);
  let conv: Conversation;
  if (mapped) {
    if (req.conversationId !== undefined && req.conversationId !== mapped.conversation_id)
      throw new Error('conversation does not belong to document');
    conv = await conversation(mapped.conversation_id);
  } else {
    if (req.conversationId !== undefined) throw new Error('document has no conversation mapping');
    creatingDocument = req;
    try {
      conv = await harness.createConversation(
        {
          ownership: { kind: "ownerless" },
          agent: {
            model: { provider: req.provider, modelId: req.modelId },
            cwd: "/doc",
          },
        },
        BACKGROUND_CONTEXT,
      );
    } finally { creatingDocument = undefined; }
    await database.run('INSERT INTO conversations_map(document_id, conversation_id) VALUES (?, ?)',
      req.documentId, conv.id);
  }
  await attachView(conv);
  const inspection = await harness.inspect(BACKGROUND_CONTEXT);
  // Already parked tasks need an actionable report on reattach too, not only
  // at the original restart-limit transition (which may precede a crash).
  for (const task of inspection.tasks) {
    if (task.record.conversationId === conv.id && task.state.kind === 'blocked'
      && task.state.reason === 'restart_limit') {
      hostEvents.onReport(JSON.stringify({ type: 'task_parked',
        message: 'Restart limit reached. Retry to continue.',
        task: { id: task.record.id, conversationId: conv.id } }));
    }
  }
  const resumable = inspection.tasks.some(task => task.record.conversationId === conv.id
    && task.state.kind !== 'blocked');
  const res: OpenDocumentRes = { conversationId: conv.id, resumable };
  return JSON.stringify(res);
}

/** Crash recovery only. Esc uses abort(), not this. */
export async function resumeWait(): Promise<void> {
  harness!.resume();
  await harness!.waitForIdle(BACKGROUND_CONTEXT);
}

export async function submit(reqJson: string): Promise<string> {
  if (!harness) throw new Error("harness closed");
  const req = JSON.parse(reqJson) as SubmitReq;
  const conv = await conversation(req.conversationId);
  await attachView(conv); // subscribe before submit — do not race first tokens
  const submission = await conv.submit(
    userInput(req),
    BACKGROUND_CONTEXT,
  );
  const settled = await submission.wait(BACKGROUND_CONTEXT);
  if (settled.status === 'unanswered') {
    hostEvents.onReport(JSON.stringify({ type: 'submission_unanswered',
      message: String(settled.detail ?? settled.reason) }));
  }
  const res: SubmitRes = {
    submissionId: submission.id,
    conversationId: conv.id,
    status: settled.status === "done" ? "done" : "unanswered",
  };
  return JSON.stringify(res);
}

export async function retryTask(taskId: number): Promise<void> {
  if (!Number.isSafeInteger(taskId) || taskId <= 0) throw new Error('invalid task id');
  await harness!.retryTask(taskId as TaskId, BACKGROUND_CONTEXT);
  await resumeWait();
}

/** After submit returns, or after trap + new instance. Not concurrent with submit. */
export async function abort(conversationId: number): Promise<void> {
  const conv = await conversation(conversationId);
  await conv.abort(BACKGROUND_CONTEXT);
}

export async function setFollow(conversationId: number, enabled: boolean): Promise<void> {
  const conv = await conversation(conversationId);
  await conv.commit(async (tx) => {
    (await tx.doc(FollowMode, conv.id)).enabled = enabled;
  }, BACKGROUND_CONTEXT);
  if (attached?.conv.id === conv.id) attached.followChanged(enabled);
}

export async function configure(conversationId: number, agentJson: string): Promise<void> {
  const conv = await conversation(conversationId);
  const change = JSON.parse(agentJson) as DurableAgentChange;
  await conv.configure(change, BACKGROUND_CONTEXT);
}

export async function close(): Promise<void> {
  attached?.dispose();
  attached = undefined;
  await harness?.close(BACKGROUND_CONTEXT);
  harness = undefined;
}

async function conversation(id: number): Promise<Conversation> {
  if (!Number.isSafeInteger(id) || id <= 0) throw new Error('invalid conversation id');
  const conv = await harness?.conversation(id as ConversationId, BACKGROUND_CONTEXT);
  if (!conv) throw new Error('unknown conversation');
  return conv;
}

async function attachView(conv: Conversation): Promise<void> {
  if (attached?.conv.id === conv.id) return;
  attached?.dispose();
  // 1.4.0's structural view mounts built-in docs only, not application docs.
  let followEnabled = (await harness!.snapshot(FollowMode, conv.id, BACKGROUND_CONTEXT))?.enabled ?? true;
  const view = await conv.viewState(BACKGROUND_CONTEXT);
  const lastEntryId = (v: typeof view.value) => {
    const entries = v.entries;
    return entries?.at(-1)?.id;
  };
  // 1.4.0 retains pi.live as {} when idle; presence is NOT busy.
  const isBusy = (value: typeof view.value) => {
    const live = value.docs['pi.live'];
    return !!(live?.run || live?.generation || (Array.isArray(live?.compactions) && live.compactions.length));
  };
  let lastHead = lastEntryId(view.value);
  let lastBusy = isBusy(view.value);
  let lastLive = view.value.docs['pi.live'];
  const emitCommit = (reason: CommitEvent["reason"], value = view.value) => {
    const busy = isBusy(value);
    const head = lastEntryId(value);
    hostEvents.onCommit(JSON.stringify({
      conversationId: conv.id,
      reason,
      busy,
      headEntryId: head,
      entries: value.entries,
      followEnabled,
    } satisfies CommitEvent));
    lastHead = head;
    lastBusy = busy;
  };
  emitCommit("other");
  const unsub = view.subscribe((value) => {
    const live = value.docs["pi.live"];
    if (live && live !== lastLive) {
      hostEvents.onStream(JSON.stringify({ conversationId: conv.id, live } satisfies StreamEvent));
    }
    lastLive = live;
    const busy = isBusy(value);
    const head = lastEntryId(value);
    if (head !== lastHead || busy !== lastBusy) {
      emitCommit("other", value);
    }
  });
  attached = {
    conv,
    followChanged: (enabled) => { followEnabled = enabled; emitCommit('other'); },
    dispose: () => {
      unsub?.();
      view.dispose();
    },
  };
}
