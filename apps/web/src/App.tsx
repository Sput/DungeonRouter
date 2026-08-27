import { useEffect, useState } from "react";

type Health = {
  service: string;
  status: "ok" | "degraded";
  database: "connected" | "unavailable";
};

export function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    fetch("/api/health")
      .then((response) => {
        if (!response.ok) throw new Error("Health check failed");
        return response.json() as Promise<Health>;
      })
      .then(setHealth)
      .catch(() => setError(true));
  }, []);

  const status = health?.status === "ok" ? "API connected" : error ? "API unavailable" : "Checking API";

  return (
    <main>
      <section className="shell" aria-labelledby="page-title">
        <div className="eyebrow">SRD 5.1 · 2014 rules</div>
        <h1 id="page-title">DungeonRouter</h1>
        <p className="lede">Fast, cited rules answers with cost-aware model routing.</p>

        <div className="status" role="status" aria-live="polite">
          <span className={health?.status === "ok" ? "dot dot--ok" : "dot"} />
          <span>{status}</span>
          {health?.database === "connected" && <span className="muted">SQLite ready</span>}
        </div>

        <form className="ask" onSubmit={(event) => event.preventDefault()}>
          <label htmlFor="question">Ask a rules question</label>
          <textarea id="question" name="question" placeholder="What does the prone condition do?" rows={4} disabled />
          <div className="actions">
            <select aria-label="Model selection" defaultValue="auto" disabled>
              <option value="auto">Auto</option>
              <option value="nano">Nano</option>
              <option value="mini">Mini</option>
              <option value="gpt-5">GPT-5</option>
            </select>
            <button type="submit" disabled>Ask</button>
          </div>
          <p className="hint">Chat and model routing arrive in the next milestone.</p>
        </form>
      </section>
    </main>
  );
}

