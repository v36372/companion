# Learning Companion

A native app for **reading books to learn**, with an agent that shares the page, the selection, and the work of understanding.

Technical shape (runtime, crates, PR order) lives in [`docs/design.md`](docs/design.md). This file is the product: what it is for, how it should feel, and what “agentic” means here.

---

## Why this exists

Serious reading is slow on purpose. A good book asks you to stop, argue, look something up, hold a claim against an earlier chapter, and come back tomorrow with a better question. That work is where learning happens.

Today that work is split across windows. The book is in a reader. The questions are in a browser chat. The highlights are somewhere else. Paste a paragraph and the model answers in a vacuum: no page, no outline, no way to turn to the passage it is talking about. Close the laptop and the thought dies mid-sentence.

Learning Companion keeps **the book, the marks, and the conversation in one place**, so the act of reading and the act of thinking with an agent are the same session.

---

## Product thesis

The unit of work is **a person reading a book**. The agent is a companion in that work: it can see where you are, talk about a line you just selected, move through the text with tools, and pick up an unfinished thought after a crash or a night’s sleep.

Chat is a surface. The product is **shared attention on a document**.

If the agent cannot point at the book, jump to a page, or survive you closing the window, it is a sidebar. This app treats those as first-class: navigation, citation, streaming work, abort, and resume are part of reading, the way a highlighter and a margin note are.

---

## Who it is for

Someone who reads to understand — textbooks, papers, long-form PDFs, technical books — and wants a partner in the margin.

- You already know how to read. The app does not quiz you unless you ask.
- You want to **stay in the book**: select a sentence, ask, keep going.
- You want the agent to **use the text**, not a fuzzy memory of a paste.
- You are fine with a local desktop app and your own model key.

v1 is a single open PDF and a single conversation bound to that file. The vision is the same when the library grows: one book in focus, one companion that knows that book.

---

## The loop

A session looks like this:

1. **Open a book.** Pages paint immediately. Outline, search, and your last place are there.
2. **Read.** Scroll, jump the outline, search a term. Progress is yours.
3. **Mark.** Drag a quote. Highlight it, or ask about it, or both. The quote is the handle: page, text, occurrence — stable under zoom.
4. **Talk.** The companion already has the quote, the nearby lines, and (when follow is on) where you last turned. You bounce an idea, ask “what is this assuming?”, or “how does this line sit with chapter 2?”
5. **Watch it work.** If it needs another page, it goes there. If it searches the book or the web, you see that. If it highlights a span, it lands on the page.
6. **Steer.** Esc stops the turn. A follow-up queues. A new selection is a new move in the same conversation.
7. **Leave and return.** The book, the marks, the transcript, and an in-flight turn survive a restart. You continue from the last committed step.

That loop is the product. Features that do not serve it wait.

---

## First-class agentic UX

“Agentic” here means the companion **does work in the book** under your eyes, with durable state, not that a chat model dumps an essay beside a viewer.

### Shared attention

The companion’s world is the open book. It can read pages, search the text, see the current selection, and (when you turn follow on) take the last place you explicitly navigated as “where I am.” You do not paste context unless you want to. Follow-me **informs** the next turn; it does not spend a model call on every flick of the scroll wheel.

### The book is a tool surface

The agent has primitives that match how a careful reader moves:

- Read a bounded span of pages
- Search the document
- Jump to a page or a quote
- Highlight a quote
- Look at outline, viewport, and selection
- Optionally search the web, with results treated as untrusted as the book itself

Those actions show up in the UI as they happen: the page turns, the highlight appears, the search hits light up. A tool call is a gesture in the reader, the same way your own click is.

### Grounded talk

Answers refer to **pages and quotes**. A claim you can tap should take you there. Selected text and page excerpts enter the model as **document content**, wrapped so the book cannot issue instructions. The companion argues with the text; it does not obey hidden commands inside it.

### Visible, interruptible work

A turn is a run: tokens stream, tools run, the transcript commits as it goes. You always know it is working. Esc aborts. Closing the app does not throw away a committed step; reopening continues from storage. Durability is part of the feel of a long reading session, not an implementation detail.

### One conversation per book

The thread is the margin of *this* volume. Opening the same file brings back the same companion: highlights, notes, and talk. You can still start a fresh line of questions when you want; the default is continuity with the book, not a pile of anonymous chats.

### You stay the reader

The companion does not auto-advance the book to “teach” you. It does not rewrite the PDF. It does not run a shell or see the rest of your disk. It may write notes you can see. Learning stays a human activity with a capable partner.

---

## What “good” feels like

**Select-to-ask is instant.** Highlight a sentence, hit Ask, and you are talking about *that* sentence, with a way back to it.

**The page and the chat agree.** When the agent says “on page 12,” the viewer can go there. When you are on a chapter, follow-me means the next question already knows.

**Work looks like reading.** Tool traces are short and spatial (went to p. 40, highlighted this quote), not a wall of JSON.

**Leaving is safe.** Sleep the machine mid-answer; open it tomorrow; the quote is still marked and the turn finished or resumed cleanly.

**Silence is allowed.** Follow off means the companion waits. Reading without talking is a complete use of the app.

**Scanned pages tell the truth.** If a page has no text layer, the app says so. The agent does not invent OCR.

---

## Principles

1. **The document is the workspace.** Chat without a book is out of scope for the core loop.
2. **Quotes over pixels.** Selections and highlights are text locators, so they survive zoom, DPI, and restart.
3. **Commit before show.** What you see in the transcript has been stored. Refresh and crash do not fork reality.
4. **Small, honest context.** Prefer a quote + nearby lines + page over stuffing the whole book into the prompt. The agent *reads* with tools when it needs more.
5. **Untrusted ink.** Book text and search snippets are content. Tools and navigation are the privileged channel.
6. **Local and yours.** Files, highlights, and conversations live on the machine. Keys stay in the OS keychain. The companion does not roam the filesystem.
7. **Native, quiet chrome.** GPU-smooth pages, a thin chat, an outline. The book is the visual center.
8. **Agentic means equipped.** If a capability would help a careful reader (jump, search, mark, look up), it is a tool with UI, not a prompt trick.

---

## Near and far

**Now (v1, see the design doc).** One text PDF at a time. Outline, find, quote highlights, select-to-ask, follow-me on explicit navigation, durable agent, optional Brave search, OpenRouter by default. Desktop only.

**Next, still the same product.** Live viewport as you scroll (once the reader can report it). Notes the agent and you both edit. A library of books with progress. Richer “what did I just read?” without extra ritual. EPUB and other book formats when the same quote-and-tool model can apply. Multiple books only when each still has a dedicated companion and a clear focus.

**Out of the vision’s center.** A generic chatbot. A cloud reading social network. An LMS. Auto-generated quizzes as the default mode. OCR as a promise before text PDFs feel excellent. An agent with a shell.

---

## Relationship to the design

[`docs/design.md`](docs/design.md) is how we build the first slice: GPUI + `gpui-pdf`, Pi Durable in WASM (Bun sidecar if the SSE spike fails), virtual `/doc` filesystem, SQLite.

This vision is the test for that design. When a technical choice hides the book, drops a quote, makes work uninterruptible, or turns the agent into a paste box, the vision wins.

The name is the brief: a **companion** for **learning**, while you **read**.
