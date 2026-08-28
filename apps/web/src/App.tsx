import { FormEvent, KeyboardEvent, useEffect, useRef, useState } from "react";

type Health = { service: string; status: "ok" | "degraded"; database: "connected" | "unavailable" };
type ModelTier = "nano" | "mini" | "gpt-5";
type RunState = "idle" | "connecting" | "streaming" | "complete" | "stopped" | "error";
type ModelDescriptor = { tier: ModelTier; label: string; model_id: string; purpose: string };
type StreamPayload =
  | { type: "metadata"; selected_model: ModelTier; upstream_model: string; route: string }
  | { type: "delta"; text: string }
  | { type: "usage"; usage: { total_tokens: number } }
  | { type: "done" };
type GroundedSource = {
  citation_id: string; chunk_id: number; document_title: string; section_path: string;
  excerpt: string; source_locator: string; source_revision: string | null; license: string | null;
};
type SourceChunk = GroundedSource & { content: string; heading: string };
type CitationValidation = { cited: string[]; unsupported: string[]; missing_required: boolean };

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
  const [model, setModel] = useState<ModelTier>("nano");
  const [answer, setAnswer] = useState("");
  const [runState, setRunState] = useState<RunState>("idle");
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [route, setRoute] = useState<string | null>(null);
  const [tokens, setTokens] = useState<number | null>(null);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [sources, setSources] = useState<GroundedSource[]>([]);
  const [citationValidation, setCitationValidation] = useState<CitationValidation | null>(null);
  const [activeSource, setActiveSource] = useState<SourceChunk | null>(null);
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
    ]).then(([nextHealth, nextModels]) => {
      setHealth(nextHealth);
      setModels(nextModels);
    }).catch(() => setHealthError(true));
  }, []);

  useEffect(() => () => controller.current?.abort(), []);
  const isRunning = runState === "connecting" || runState === "streaming";
  const status = health?.status === "ok" ? "API connected" : healthError ? "API unavailable" : "Checking API";
  const chosenModel = models.find((item) => item.tier === model);

  async function ask(event?: FormEvent) {
    event?.preventDefault();
    if (!question.trim() || isRunning) return;
    controller.current?.abort();
    const abortController = new AbortController();
    controller.current = abortController;
    const startedAt = performance.now();
    setAnswer(""); setSelectedModel(null); setRoute(null); setTokens(null); setElapsedMs(null); setMessage(null);
    setSources([]); setCitationValidation(null); setActiveSource(null);
    setRunState("connecting");
    try {
      const response = await fetch("/api/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
        body: JSON.stringify({ prompt: question.trim(), model, max_output_tokens: 800 }),
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
        if (payload.type === "metadata") { setSelectedModel(payload.upstream_model); setRoute(payload.route); }
        else if (payload.type === "delta") setAnswer((current) => current + payload.text);
        else if (payload.type === "usage") setTokens(payload.usage.total_tokens);
      });
      setElapsedMs(Math.round(performance.now() - startedAt));
      setRunState("complete");
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

  return <main><section className="shell" aria-labelledby="page-title">
    <header className="hero"><div><div className="eyebrow">SRD 5.1 · 2014 rules</div><h1 id="page-title">DungeonRouter</h1><p className="lede">Fast rules answers with cost-aware model routing.</p></div>
      <div className="status" role="status" aria-live="polite"><span className={health?.status === "ok" ? "dot dot--ok" : "dot"}/><span>{status}</span>{health?.database === "connected" && <span className="muted">SQLite ready</span>}</div></header>
    <form className="ask" onSubmit={(event) => void ask(event)}><label htmlFor="question">Query the archive</label>
      <textarea id="question" name="question" placeholder="What does the prone condition do?" rows={5} value={question} onChange={(event) => setQuestion(event.target.value)} onKeyDown={handleQuestionKeyDown} disabled={isRunning}/>
      <div className="route-picker"><div><span className="field-label">Attunement</span><p>{chosenModel?.purpose}</p></div>
        <select aria-label="Model selection" value={model} onChange={(event) => setModel(event.target.value as ModelTier)} disabled={isRunning}>{models.map((item) => <option key={item.tier} value={item.tier}>{item.label} · {item.model_id}</option>)}</select></div>
      <div className="actions"><span className="hint">Enter to ask · Shift+Enter for a new line</span>{isRunning ? <button type="button" className="stop" onClick={() => controller.current?.abort()}>Stop</button> : <button type="submit" disabled={!question.trim() || health?.status !== "ok"}>Ask</button>}</div>
    </form>
    <section className="result" aria-labelledby="answer-heading" aria-busy={isRunning}><div className="result-heading"><h2 id="answer-heading">Ruling</h2><span className={`run-state run-state--${runState}`}>{runState}</span></div>
      <div className={answer ? "answer" : "answer answer--empty"} aria-live="polite">{answer ? renderAnswer(answer, sources, citationValidation, openSource) : (isRunning ? "Consulting the archive…" : "The archive awaits your question.")}{runState === "streaming" && <span className="cursor" aria-hidden="true"/>}</div>
      {citationValidation?.unsupported.length ? <p className="message message--error">Unsupported citation markers were left unlinked: {citationValidation.unsupported.join(", ")}</p> : null}
      {citationValidation?.missing_required ? <p className="message message--error">This answer did not cite its retrieved evidence. Treat it as unverified.</p> : null}
      {message && <p className={runState === "error" ? "message message--error" : "message"}>{message}</p>}
      <dl className="metadata"><div><dt>Model</dt><dd>{selectedModel ?? "—"}</dd></div><div><dt>Route</dt><dd>{route ?? "—"}</dd></div><div><dt>Tokens</dt><dd>{tokens ?? "—"}</dd></div><div><dt>Time</dt><dd>{elapsedMs === null ? "—" : `${(elapsedMs / 1000).toFixed(1)}s`}</dd></div></dl>
    </section>
    {sources.length > 0 && <section className="sources" aria-labelledby="sources-heading"><div className="sources-heading"><h2 id="sources-heading">Sources</h2><span>{sources.length} retrieved</span></div>
      <div className="source-list">{sources.map((source) => <button type="button" className="source-card" key={source.citation_id} onClick={() => void openSource(source)}><span className="source-id">{source.citation_id}</span><span><strong>{source.section_path}</strong><small>{source.excerpt}</small></span></button>)}</div>
      {activeSource && <article className="source-detail"><div><span className="source-id">{activeSource.citation_id}</span><strong>{activeSource.section_path}</strong><button type="button" className="close-source" aria-label="Close source" onClick={() => setActiveSource(null)}>×</button></div><pre>{activeSource.content}</pre><footer>{activeSource.source_locator} · {activeSource.license}</footer></article>}
    </section>}
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
