import { FormEvent, KeyboardEvent, useEffect, useRef, useState } from "react";

type Health = { service: string; status: "ok" | "degraded"; database: "connected" | "unavailable" };
type ModelTier = "nano" | "mini" | "gpt-5";
type RoutingChoice = "auto" | ModelTier;
type RunState = "idle" | "connecting" | "streaming" | "complete" | "stopped" | "error";
type ModelDescriptor = { tier: ModelTier; label: string; model_id: string; purpose: string };
type StreamPayload =
  | { type: "metadata"; requested_model: ModelTier; selected_model: string; routing_mode: "auto" | "manual"; route: string; routing_reason: string | null; classifier_confidence: number | null }
  | { type: "delta"; text: string }
  | { type: "usage"; usage: { total_tokens: number } }
  | { type: "done" };
type GroundedSource = {
  citation_id: string; chunk_id: number; kind: "srd" | "campaign_note"; document_title: string; section_path: string;
  excerpt: string; source_locator: string; source_revision: string | null; license: string | null;
};
type SourceChunk = GroundedSource & { content: string; heading: string };
type CitationValidation = { cited: string[]; unsupported: string[]; missing_required: boolean };
type CampaignNote = { id: number; title: string; filename: string; chunk_count: number; created_at: string };
type UsageSummary = { total_questions: number; actual_cost_usd: number; always_gpt5_cost_usd: number; estimated_savings_usd: number; estimated_savings_percent: number; automatic_requests: number; manual_requests: number; requests_by_model: { model: string; requests: number; average_latency_ms: number }[]; warning_threshold_usd: number; hard_limit_usd: number; budget_status: "ok" | "warning" | "stopped"; pricing_version: string };

const FALLBACK_MODELS: ModelDescriptor[] = [
  { tier: "nano", label: "Nano", model_id: "gpt-5-nano", purpose: "Direct lookup and short grounded answers" },
  { tier: "mini", label: "Mini", model_id: "gpt-5-mini", purpose: "Multi-rule explanation and ordinary adjudication" },
  { tier: "gpt-5", label: "GPT-5", model_id: "gpt-5", purpose: "Difficult reasoning and ambiguous interactions" },
];

export function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [healthError, setHealthError] = useState(false);
  const [models, setModels] = useState(FALLBACK_MODELS);
  const [question, setQuestion] = useState("");
  const [model, setModel] = useState<RoutingChoice>("auto");
  const [answer, setAnswer] = useState("");
  const [runState, setRunState] = useState<RunState>("idle");
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [route, setRoute] = useState<string | null>(null);
  const [routingReason, setRoutingReason] = useState<string | null>(null);
  const [classifierConfidence, setClassifierConfidence] = useState<number | null>(null);
  const [tokens, setTokens] = useState<number | null>(null);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [sources, setSources] = useState<GroundedSource[]>([]);
  const [citationValidation, setCitationValidation] = useState<CitationValidation | null>(null);
  const [activeSource, setActiveSource] = useState<SourceChunk | null>(null);
  const [notes, setNotes] = useState<CampaignNote[]>([]);
  const [noteMessage, setNoteMessage] = useState<string | null>(null);
  const [notesBusy, setNotesBusy] = useState(false);
  const [usageSummary, setUsageSummary] = useState<UsageSummary | null>(null);
  const controller = useRef<AbortController | null>(null);

  useEffect(() => {
    Promise.all([
      fetch("/api/health").then((response) => {
        if (!response.ok) throw new Error("Health check failed");
        return response.json() as Promise<Health>;
      }),
      fetch("/api/models").then((response) => {
        if (!response.ok) throw new Error("Model catalog failed");
        return response.json() as Promise<ModelDescriptor[]>;
      }),
      fetch("/api/notes").then((response) => response.ok ? response.json() as Promise<CampaignNote[]> : []),
    ]).then(([nextHealth, nextModels, nextNotes]) => {
      setHealth(nextHealth);
      setModels(nextModels);
      setNotes(nextNotes);
    }).catch(() => setHealthError(true));
  }, []);

  useEffect(() => () => controller.current?.abort(), []);
  useEffect(() => { void refreshUsage(); }, []);
  const isRunning = runState === "connecting" || runState === "streaming";
  const status = health?.status === "ok" ? "API connected" : healthError ? "API unavailable" : "Checking API";
  const chosenModel = model === "auto" ? null : models.find((item) => item.tier === model);

  async function refreshUsage() {
    const response = await fetch("/api/usage/summary").catch(() => null);
    if (response?.ok) setUsageSummary(await response.json() as UsageSummary);
  }

  async function ask(event?: FormEvent) {
    event?.preventDefault();
    if (!question.trim() || isRunning || usageSummary?.budget_status === "stopped") return;
    controller.current?.abort();
    const abortController = new AbortController();
    controller.current = abortController;
    const startedAt = performance.now();
    setAnswer(""); setSelectedModel(null); setRoute(null); setRoutingReason(null); setClassifierConfidence(null); setTokens(null); setElapsedMs(null); setMessage(null);
    setSources([]); setCitationValidation(null); setActiveSource(null);
    setRunState("connecting");
    try {
      const response = await fetch("/api/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
        body: JSON.stringify({ prompt: question.trim(), model: model === "auto" ? "mini" : model, routing_mode: model === "auto" ? "auto" : "manual", max_output_tokens: 800 }),
        signal: abortController.signal,
      });
      if (!response.ok) {
        const body = (await response.json().catch(() => null)) as { error?: string } | null;
        throw new Error(body?.error ?? `Request failed (${response.status})`);
      }
      if (!response.body) throw new Error("This browser did not provide a response stream");
      setRunState("streaming");
      await readEventStream(response.body, (eventName, data) => {
        if (eventName === "error") {
          const error = JSON.parse(data) as { error?: string };
          throw new Error(error.error ?? "The model stream failed");
        }
        if (eventName === "sources") {
          setSources(JSON.parse(data) as GroundedSource[]);
          return;
        }
        if (eventName === "citation_validation") {
          setCitationValidation(JSON.parse(data) as CitationValidation);
          return;
        }
        const payload = JSON.parse(data) as StreamPayload;
        if (payload.type === "metadata") { setSelectedModel(payload.selected_model); setRoute(payload.route); setRoutingReason(payload.routing_reason); setClassifierConfidence(payload.classifier_confidence); }
        else if (payload.type === "delta") setAnswer((current) => current + payload.text);
        else if (payload.type === "usage") setTokens(payload.usage.total_tokens);
      });
      setElapsedMs(Math.round(performance.now() - startedAt));
      setRunState("complete");
      await refreshUsage();
    } catch (error) {
      setElapsedMs(Math.round(performance.now() - startedAt));
      if (abortController.signal.aborted) {
        setRunState("stopped"); setMessage("Generation stopped. The partial answer is preserved.");
      } else {
        setRunState("error"); setMessage(error instanceof Error ? error.message : "The request failed");
      }
    } finally {
      if (controller.current === abortController) controller.current = null;
    }
  }

  function handleQuestionKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void ask(); }
  }

  async function openSource(source: GroundedSource) {
    try {
      const response = await fetch(`/api/sources/${source.chunk_id}`);
      if (!response.ok) throw new Error("Source unavailable");
      setActiveSource({ ...(await response.json()), citation_id: source.citation_id, excerpt: source.excerpt } as SourceChunk);
    } catch {
      setMessage("The cited source passage could not be loaded.");
    }
  }

  async function uploadNote(file: File | undefined) {
    if (!file) return;
    setNotesBusy(true); setNoteMessage(null);
    try {
      if (!/\.(md|markdown|txt)$/i.test(file.name)) throw new Error("Choose a Markdown or plain-text file.");
      if (file.size > 256 * 1024) throw new Error("Campaign notes must be no larger than 256 KiB.");
      const response = await fetch("/api/notes", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ title: file.name.replace(/\.(md|markdown|txt)$/i, ""), filename: file.name, content: await file.text() }),
      });
      const body = await response.json().catch(() => null) as CampaignNote & { error?: string } | null;
      if (!response.ok) throw new Error(body?.error ?? "The note could not be indexed.");
      setNotes((current) => [body as CampaignNote, ...current]);
      setNoteMessage(`${file.name} is indexed and available to answers.`);
    } catch (error) {
      setNoteMessage(error instanceof Error ? error.message : "The note could not be indexed.");
    } finally { setNotesBusy(false); }
  }

  async function deleteNote(note: CampaignNote) {
    if (!window.confirm(`Delete “${note.title}” and all of its indexed passages?`)) return;
    setNotesBusy(true); setNoteMessage(null);
    try {
      const response = await fetch(`/api/notes/${note.id}`, { method: "DELETE" });
      if (!response.ok) throw new Error("The note could not be deleted.");
      setNotes((current) => current.filter((item) => item.id !== note.id));
      setNoteMessage(`${note.title} was deleted.`);
    } catch (error) {
      setNoteMessage(error instanceof Error ? error.message : "The note could not be deleted.");
    } finally { setNotesBusy(false); }
  }

  return <main><section className="shell" aria-labelledby="page-title">
    <header className="hero"><div><div className="eyebrow">SRD 5.1 · 2014 rules</div><h1 id="page-title">DungeonRouter</h1><p className="lede">Fast rules answers with cost-aware model routing.</p></div>
      <div className="status" role="status" aria-live="polite"><span className={health?.status === "ok" ? "dot dot--ok" : "dot"}/><span>{status}</span>{health?.database === "connected" && <span className="muted">SQLite ready</span>}</div></header>
    <form className="ask" onSubmit={(event) => void ask(event)}><label htmlFor="question">Query the archive</label>
      <textarea id="question" name="question" placeholder="What does the prone condition do?" rows={5} value={question} onChange={(event) => setQuestion(event.target.value)} onKeyDown={handleQuestionKeyDown} disabled={isRunning}/>
      <div className="route-picker"><div><span className="field-label">Attunement</span><p>{model === "auto" ? "Switchyard selects the cheapest capable model" : chosenModel?.purpose}</p></div>
        <select aria-label="Model selection" value={model} onChange={(event) => setModel(event.target.value as RoutingChoice)} disabled={isRunning}><option value="auto">Auto · cost-aware</option>{models.map((item) => <option key={item.tier} value={item.tier}>{item.label} · {item.model_id}</option>)}</select></div>
      <div className="actions"><span className="hint">{runState === "connecting" ? "Retrieving sources and selecting a route…" : runState === "streaming" ? "Streaming the selected model’s answer…" : "Enter to ask · Shift+Enter for a new line"}</span>{isRunning ? <button type="button" className="stop" onClick={() => controller.current?.abort()}>Stop</button> : <button type="submit" disabled={!question.trim() || health?.status !== "ok" || usageSummary?.budget_status === "stopped"}>Ask</button>}</div>
      {usageSummary?.budget_status === "stopped" && <p className="budget-blocked" role="alert">The monthly model-cost limit has been reached. Increase the local limit and restart the API to make another model call.</p>}
    </form>
    <section className="result" aria-labelledby="answer-heading" aria-busy={isRunning}><div className="result-heading"><h2 id="answer-heading">Ruling</h2><span className={`run-state run-state--${runState}`}>{runState}</span></div>
      <div className={answer ? "answer" : "answer answer--empty"} aria-live="polite">{answer ? renderAnswer(answer, sources, citationValidation, openSource) : (isRunning ? "Consulting the archive…" : "The archive awaits your question.")}{runState === "streaming" && <span className="cursor" aria-hidden="true"/>}</div>
      {citationValidation?.unsupported.length ? <p className="message message--error">Unsupported citation markers were left unlinked: {citationValidation.unsupported.join(", ")}</p> : null}
      {citationValidation?.missing_required ? <p className="message message--error">This answer did not cite its retrieved evidence. Treat it as unverified.</p> : null}
      {message && <p className={runState === "error" ? "message message--error" : "message"}>{message}</p>}
      <dl className="metadata"><div><dt>Model</dt><dd>{selectedModel ?? "—"}</dd></div><div><dt>Route</dt><dd>{route ?? "—"}</dd></div><div><dt>Tokens</dt><dd>{tokens ?? "—"}</dd></div><div><dt>Time</dt><dd>{elapsedMs === null ? "—" : `${(elapsedMs / 1000).toFixed(1)}s`}</dd></div></dl>
      {routingReason && <p className="routing-reason"><strong>Why this model:</strong> {routingReason}{classifierConfidence !== null ? ` · ${Math.round(classifierConfidence * 100)}% confidence` : ""}</p>}
    </section>
    {sources.length > 0 && <section className="sources" aria-labelledby="sources-heading"><div className="sources-heading"><h2 id="sources-heading">Sources</h2><span>{sources.length} retrieved</span></div>
      <div className="source-list">{sources.map((source) => <button type="button" className="source-card" key={source.citation_id} onClick={() => void openSource(source)}><span className="source-id">{source.citation_id}</span><span><span className={`source-kind source-kind--${source.kind}`}>{source.kind === "srd" ? "SRD" : "Campaign"}</span><strong>{source.section_path}</strong><small>{source.excerpt}</small></span></button>)}</div>
      {activeSource && <article className="source-detail"><div><span className="source-id">{activeSource.citation_id}</span><strong>{activeSource.section_path}</strong><button type="button" className="close-source" aria-label="Close source" onClick={() => setActiveSource(null)}>×</button></div><pre>{activeSource.content}</pre><footer>{activeSource.source_locator} · {activeSource.kind === "campaign_note" ? "Private campaign note" : activeSource.license}</footer></article>}
    </section>}
    <section className="notes" aria-labelledby="notes-heading"><div className="notes-heading"><div><h2 id="notes-heading">Campaign notes</h2><p>Private local context sent to OpenAI only when retrieved for an answer.</p></div><label className="upload-button">{notesBusy ? "Working…" : "Add note"}<input type="file" accept=".md,.markdown,.txt,text/plain,text/markdown" disabled={notesBusy} onChange={(event) => { void uploadNote(event.target.files?.[0]); event.currentTarget.value = ""; }}/></label></div>
      {noteMessage && <p className="note-message" role="status">{noteMessage}</p>}
      {notes.length ? <ul className="note-list">{notes.map((note) => <li key={note.id}><span><strong>{note.title}</strong><small>{note.filename} · {note.chunk_count} {note.chunk_count === 1 ? "passage" : "passages"}</small></span><button type="button" className="delete-note" disabled={notesBusy} onClick={() => void deleteNote(note)}>Delete</button></li>)}</ul> : <p className="notes-empty">No campaign notes indexed yet. Markdown and text files up to 256 KiB are supported.</p>}
    </section>
    <section className="usage" aria-labelledby="usage-heading"><div className="usage-heading"><div><h2 id="usage-heading">Monthly usage</h2><p>Estimated from API token counts; provider billing may differ.</p></div><span className={`budget budget--${usageSummary?.budget_status ?? "ok"}`}>{usageSummary?.budget_status ?? "loading"}</span></div>
      <dl className="usage-grid"><div><dt>Questions</dt><dd>{usageSummary?.total_questions ?? "—"}</dd></div><div><dt>Routed cost</dt><dd>{usageSummary ? `$${usageSummary.actual_cost_usd.toFixed(4)}` : "—"}</dd></div><div><dt>Always GPT-5</dt><dd>{usageSummary ? `$${usageSummary.always_gpt5_cost_usd.toFixed(4)}` : "—"}</dd></div><div><dt>Savings</dt><dd>{usageSummary ? `${usageSummary.estimated_savings_percent.toFixed(1)}%` : "—"}</dd></div></dl>
      {usageSummary && <><div className="budget-track"><span style={{ width: `${Math.min(100, usageSummary.actual_cost_usd / usageSummary.hard_limit_usd * 100)}%` }}/></div><p className="budget-copy">${usageSummary.actual_cost_usd.toFixed(4)} of ${usageSummary.hard_limit_usd.toFixed(2)} hard limit · warning at ${usageSummary.warning_threshold_usd.toFixed(2)} · {usageSummary.automatic_requests} auto / {usageSummary.manual_requests} manual</p><div className="model-counts">{usageSummary.requests_by_model.map((item) => <span key={item.model}>{item.model}: {item.requests} · {(item.average_latency_ms / 1000).toFixed(1)}s avg</span>)}</div></>}
    </section>
  </section></main>;
}

function renderAnswer(answer: string, sources: GroundedSource[], validation: CitationValidation | null, openSource: (source: GroundedSource) => Promise<void>) {
  const sourceById = new Map(sources.map((source) => [source.citation_id, source]));
  const valid = new Set(validation?.cited ?? []);
  return answer.split(/(\[S\d+\])/g).map((part, index) => {
    const citationId = part.match(/^\[(S\d+)\]$/)?.[1];
    const source = citationId ? sourceById.get(citationId) : undefined;
    return source && valid.has(citationId!)
      ? <button type="button" className="citation" key={`${part}-${index}`} onClick={() => void openSource(source)}>{part}</button>
      : part;
  });
}

async function readEventStream(stream: ReadableStream<Uint8Array>, onEvent: (event: string, data: string) => void) {
  const reader = stream.getReader(); const decoder = new TextDecoder(); let buffer = "";
  while (true) {
    const { done, value } = await reader.read();
    buffer += decoder.decode(value, { stream: !done }).replaceAll("\r\n", "\n");
    let boundary = buffer.indexOf("\n\n");
    while (boundary >= 0) {
      const frame = buffer.slice(0, boundary); buffer = buffer.slice(boundary + 2);
      const lines = frame.split("\n");
      const event = lines.find((line) => line.startsWith("event:"))?.slice(6).trim() ?? "message";
      const data = lines.filter((line) => line.startsWith("data:")).map((line) => line.slice(5).trimStart()).join("\n");
      if (data) onEvent(event, data);
      boundary = buffer.indexOf("\n\n");
    }
    if (done) break;
  }
}
