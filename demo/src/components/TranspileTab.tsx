import { useState, useEffect, useCallback } from 'react';
import { scenarios, type Scenario } from '../data/scenarios';

interface WasmModule {
  parse: (input: string) => string;
  to_logql: (input: string) => string;
  to_logsql: (input: string) => string;
  to_promql: (input: string) => string;
  validate: (input: string) => string;
  explain?: (input: string) => string;
}

interface Props {
  wasm: unknown;
  wasmLoading: boolean;
  transpile: (q: string, b: string) => string | null;
}

interface ExplainStep {
  step: number;
  action: string;
  detail: string;
  native_query?: string;
  backend?: string;
}

const ENGINE_URL = `http://${window.location.hostname}:9090`;
const backendLabels: Record<string, string> = { promql: 'PromQL', logql: 'LogQL', logsql: 'LogsQL' };
const allBackends = ['promql', 'logql', 'logsql'] as const;

export default function TranspileTab({ wasm, wasmLoading, transpile }: Props) {
  const [query, setQuery] = useState(scenarios[0].query);
  const [scenario, setScenario] = useState<Scenario>(scenarios[0]);
  const [results, setResults] = useState<Record<string, string>>({});
  const [parseTime, setParseTime] = useState<number | null>(null);
  const [explainSteps, setExplainSteps] = useState<ExplainStep[] | null>(null);
  const [explainLoading, setExplainLoading] = useState(false);

  const runTranspile = useCallback(() => {
    if (!wasm || !query.trim()) { setResults({}); setParseTime(null); return; }
    const start = performance.now();
    const r: Record<string, string> = {};
    for (const b of allBackends) {
      try { r[b] = transpile(query, b) || ''; } catch (e) { r[b] = `Error: ${e}`; }
    }
    setParseTime(performance.now() - start);
    setResults(r);
  }, [wasm, query, transpile]);

  useEffect(() => { runTranspile(); }, [runTranspile]);

  const pick = (s: Scenario) => { setScenario(s); setQuery(s.query); setExplainSteps(null); };

  const runExplain = async () => {
    if (!query.trim()) return;
    setExplainLoading(true);
    try {
      // Try server-side explain first
      const resp = await fetch(`${ENGINE_URL}/v1/explain`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query: query.replace(/\n/g, ' ').trim() }),
      });
      const json = await resp.json();
      setExplainSteps(json.plan?.steps || []);
    } catch {
      // Fallback to WASM explain if engine unreachable
      const mod = wasm as WasmModule | null;
      if (mod?.explain) {
        try {
          const result = JSON.parse(mod.explain(query.replace(/\n/g, ' ').trim()));
          setExplainSteps(result.steps || []);
        } catch { setExplainSteps([]); }
      } else {
        setExplainSteps([]);
      }
    }
    setExplainLoading(false);
  };

  return (
    <div className="space-y-4 pt-4 animate-fade-in">
      {/* Scenario buttons */}
      <div className="flex items-center gap-2 flex-wrap">
        <span className="text-xs text-[var(--color-text-dim)] uppercase tracking-wider mr-1">Scenarios:</span>
        {scenarios.map((s) => (
          <button
            key={s.id}
            onClick={() => pick(s)}
            className={`px-3 py-1 rounded text-xs font-medium transition-all cursor-pointer border ${
              scenario.id === s.id
                ? 'border-[var(--color-accent)]/40 bg-[var(--color-accent-dim)] text-[var(--color-accent)]'
                : 'border-[var(--color-border)] text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)]'
            }`}
          >
            {s.icon} {s.title}
          </button>
        ))}
        {parseTime !== null && (
          <span className="ml-auto text-[10px] text-[var(--color-green)] font-mono">
            {parseTime < 1 ? `${(parseTime * 1000).toFixed(0)}us` : `${parseTime.toFixed(2)}ms`} all 3 backends
          </span>
        )}
      </div>

      {/* Editor + outputs */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {/* UNIQL input */}
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
          <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            <span className="text-xs font-semibold text-[var(--color-accent)] tracking-wider">UNIQL INPUT</span>
            <span className="text-[10px] text-[var(--color-text-dim)]">{scenario.icon} {scenario.description}</span>
          </div>
          <textarea
            value={query}
            onChange={(e) => { setQuery(e.target.value); setExplainSteps(null); }}
            className="w-full p-4 bg-transparent text-[var(--color-accent)] font-mono text-sm resize-none focus:outline-none leading-relaxed min-h-[200px]"
            spellCheck={false}
          />
          <div className="px-4 pb-3 flex items-center gap-2">
            <button
              onClick={runExplain}
              disabled={explainLoading || !query.trim()}
              className="px-3 py-1 rounded text-[10px] font-semibold cursor-pointer transition-all border border-[var(--color-accent)]/30 bg-[var(--color-accent-dim)] text-[var(--color-accent)] hover:bg-[var(--color-accent)]/20 disabled:opacity-40"
            >
              {explainLoading ? 'Explaining...' : 'Explain Plan'}
            </button>
          </div>
          {explainSteps && explainSteps.length > 0 && (
            <div className="border-t border-[var(--color-border)] px-4 py-3 space-y-1.5">
              <div className="text-[10px] font-semibold text-[var(--color-green)] uppercase tracking-wider mb-2">Execution Plan</div>
              {explainSteps.map((s) => (
                <div key={s.step} className="flex items-start gap-2 text-[10px]">
                  <span className="w-4 h-4 rounded-full bg-[var(--color-surface-3)] flex items-center justify-center text-[8px] font-bold text-[var(--color-text-dim)] shrink-0 mt-0.5">{s.step}</span>
                  <div className="min-w-0">
                    <span className="text-[var(--color-accent)] font-mono">{s.action}</span>
                    <span className="text-[var(--color-text-dim)] ml-2">{s.detail}</span>
                    {s.native_query && (
                      <div className="text-[var(--color-cyan)] font-mono break-all mt-0.5">{s.native_query}</div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* All 3 outputs */}
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
          <div className="px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            <span className="text-xs font-semibold text-[var(--color-cyan)] tracking-wider">NATIVE OUTPUTS</span>
          </div>
          <div className="divide-y divide-[var(--color-border)]">
            {allBackends.map((b) => {
              const r = results[b];
              const isError = r?.startsWith('Error');
              const isActive = b === scenario.backend;
              return (
                <div key={b} className={`p-3 ${isActive ? 'bg-[var(--color-surface-3)]' : ''}`}>
                  <div className="flex items-center gap-2 mb-1">
                    <span className={`text-[10px] font-semibold uppercase tracking-wider ${isActive ? 'text-[var(--color-cyan)]' : 'text-[var(--color-text-dim)]'}`}>
                      {backendLabels[b]}
                    </span>
                    {isActive && <span className="text-[10px] px-1.5 py-0.5 rounded bg-[var(--color-cyan-dim)] text-[var(--color-cyan)]">TARGET</span>}
                  </div>
                  <pre className={`text-xs font-mono leading-relaxed break-all ${isError ? 'text-[var(--color-red)]' : 'text-[var(--color-text)]'}`}>
                    {wasmLoading ? 'Loading WASM...' : r || '--'}
                  </pre>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
