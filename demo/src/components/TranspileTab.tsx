import { useState, useEffect, useCallback, useRef } from 'react';
import { scenarios, type Scenario } from '../data/scenarios';

interface WasmModule {
  parse: (input: string) => string;
  to_logql: (input: string) => string;
  to_logsql: (input: string) => string;
  to_promql: (input: string) => string;
  validate: (input: string) => string;
  explain?: (input: string) => string;
  autocomplete?: (input: string, cursor: number) => string;
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

interface ValidationResult {
  valid: boolean;
  error?: string;
  signals?: string[];
  clauses?: string;
  warnings?: string[];
}

interface LiveResult {
  status: string;
  data: unknown;
  metadata: { native_query: string; total_time_ms: number; backend: string; backend_type: string; signal_type: string };
}

const ENGINE_URL = `http://${window.location.hostname}:9090`;
const allBackends = ['promql', 'logql', 'logsql'] as const;
const backendMeta: Record<string, { label: string; sub: string; color: string }> = {
  promql: { label: 'PromQL', sub: 'VictoriaMetrics / Prometheus', color: 'var(--color-accent)' },
  logql: { label: 'LogQL', sub: 'Grafana Loki', color: 'var(--color-green)' },
  logsql: { label: 'LogsQL', sub: 'VictoriaLogs', color: 'var(--color-amber)' },
};

function highlightUniql(code: string): string {
  return code
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/("(?:[^"\\]|\\.)*")/g, '<span style="color:var(--color-green)">$1</span>')
    .replace(/\b(SHOW|FROM|WHERE|AND|OR|NOT|WITHIN|COMPUTE|GROUP BY|HAVING|CORRELATE|ON|PARSE|DEFINE|AS|IN|CONTAINS|MATCHES|STARTS WITH|NATIVE)\b/gi, '<span style="color:var(--color-accent)">$1</span>')
    .replace(/\b(count|sum|avg|min|max|rate|irate|increase|p50|p90|p95|p99|json|logfmt|pattern|regexp)\b/gi, '<span style="color:var(--color-cyan)">$1</span>')
    .replace(/\b(timeseries|table|count|timeline|heatmap|last|today|this_week)\b/gi, '<span style="color:var(--color-amber)">$1</span>');
}

export default function TranspileTab({ wasm, wasmLoading, transpile }: Props) {
  const [query, setQuery] = useState(scenarios[0].query);
  const [scenario, setScenario] = useState<Scenario>(scenarios[0]);
  const [results, setResults] = useState<Record<string, string>>({});
  const [parseTime, setParseTime] = useState<number | null>(null);
  const [validation, setValidation] = useState<ValidationResult | null>(null);
  const [explainSteps, setExplainSteps] = useState<ExplainStep[] | null>(null);
  const [explainLoading, setExplainLoading] = useState(false);
  const [liveResult, setLiveResult] = useState<LiveResult | null>(null);
  const [liveLoading, setLiveLoading] = useState(false);
  const [activePanel, setActivePanel] = useState<'transpile' | 'explain' | 'execute'>('transpile');
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [showSuggestions, setShowSuggestions] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const runTranspile = useCallback(() => {
    if (!wasm || !query.trim()) { setResults({}); setParseTime(null); setValidation(null); return; }
    const start = performance.now();
    const r: Record<string, string> = {};
    for (const b of allBackends) {
      try { r[b] = transpile(query, b) || ''; } catch (e) { r[b] = `Error: ${e}`; }
    }
    setParseTime(performance.now() - start);
    setResults(r);
    const mod = wasm as WasmModule;
    try { setValidation(JSON.parse(mod.validate(query.replace(/\n/g, ' ').trim()))); } catch { setValidation(null); }
  }, [wasm, query, transpile]);

  useEffect(() => { runTranspile(); }, [runTranspile]);

  const handleQueryChange = (value: string) => {
    setQuery(value);
    setExplainSteps(null);
    setLiveResult(null);
    setActivePanel('transpile');
    const mod = wasm as WasmModule | null;
    if (mod?.autocomplete && textareaRef.current) {
      try {
        const cursor = textareaRef.current.selectionStart || value.length;
        const result = JSON.parse(mod.autocomplete(value.replace(/\n/g, ' '), cursor));
        setSuggestions(result.suggestions || []);
        setShowSuggestions((result.suggestions || []).length > 0 && value.endsWith(' '));
      } catch { setSuggestions([]); setShowSuggestions(false); }
    }
  };

  const applySuggestion = (s: string) => {
    const parts = query.trimEnd().split(/\s+/);
    const last = parts[parts.length - 1];
    if (last && s.toLowerCase().startsWith(last.toLowerCase())) parts[parts.length - 1] = s;
    else parts.push(s);
    setQuery(parts.join(' ') + ' ');
    setShowSuggestions(false);
    textareaRef.current?.focus();
  };

  const pick = (s: Scenario) => { setScenario(s); setQuery(s.query); setExplainSteps(null); setLiveResult(null); setActivePanel('transpile'); };

  const runExplain = async () => {
    setExplainLoading(true);
    setActivePanel('explain');
    try {
      const resp = await fetch(`${ENGINE_URL}/v1/explain`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: query.replace(/\n/g, ' ').trim() }) });
      const json = await resp.json();
      setExplainSteps(json.plan?.steps || []);
    } catch {
      const mod = wasm as WasmModule | null;
      if (mod?.explain) { try { setExplainSteps(JSON.parse(mod.explain(query.replace(/\n/g, ' ').trim())).steps || []); } catch { setExplainSteps([]); } }
    }
    setExplainLoading(false);
  };

  const runLive = async () => {
    setLiveLoading(true);
    setActivePanel('execute');
    try {
      const resp = await fetch(`${ENGINE_URL}/v1/query`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ query: query.replace(/\n/g, ' ').trim(), limit: 20 }) });
      const json = await resp.json();
      setLiveResult(json);
    } catch { setLiveResult(null); }
    setLiveLoading(false);
  };

  return (
    <div className="space-y-4 pt-4 animate-fade-in">
      {/* Scenarios */}
      <div className="flex items-center gap-2 flex-wrap">
        <span className="text-[11px] text-[var(--color-text-dim)] uppercase tracking-wider mr-1">Examples:</span>
        {scenarios.map((s) => (
          <button key={s.id} onClick={() => pick(s)}
            className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-all cursor-pointer border ${
              scenario.id === s.id ? 'border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 text-[var(--color-accent)]' : 'border-[var(--color-border)] text-[var(--color-text-dim)] hover:text-[var(--color-text)] hover:bg-[var(--color-surface-2)]'
            }`}
          >{s.icon} {s.title}</button>
        ))}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-5 gap-4">
        {/* ─── Left: Editor (3 col) ─── */}
        <div className="lg:col-span-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
          {/* Editor header */}
          <div className="flex items-center justify-between px-4 py-2.5 border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            <div className="flex items-center gap-2">
              <span className="w-3 h-3 rounded-full bg-[#ff5f57]" />
              <span className="w-3 h-3 rounded-full bg-[#febc2e]" />
              <span className="w-3 h-3 rounded-full bg-[#28c840]" />
              <span className="ml-2 text-[11px] text-[var(--color-text-dim)] font-mono">uniql-playground</span>
            </div>
            <div className="flex items-center gap-2">
              {validation && (
                <span className={`text-[9px] px-2 py-0.5 rounded-full font-semibold ${
                  validation.valid ? 'bg-[var(--color-green)]/10 text-[var(--color-green)]' : 'bg-[var(--color-red)]/10 text-[var(--color-red)]'
                }`}>{validation.valid ? 'VALID' : 'ERROR'}</span>
              )}
              {validation?.valid && validation.clauses && (
                <span className="text-[9px] text-[var(--color-text-dim)] font-mono">{validation.clauses}</span>
              )}
              {parseTime !== null && (
                <span className="text-[9px] text-[var(--color-green)] font-mono">{parseTime < 1 ? `${(parseTime * 1000).toFixed(0)}us` : `${parseTime.toFixed(1)}ms`}</span>
              )}
            </div>
          </div>

          {/* Syntax-highlighted editor */}
          <div className="relative min-h-[180px]">
            <pre className="p-4 font-mono text-[13px] leading-relaxed whitespace-pre-wrap break-words pointer-events-none" aria-hidden="true" dangerouslySetInnerHTML={{ __html: highlightUniql(query) + '\n' }} />
            <textarea ref={textareaRef} value={query} onChange={(e) => handleQueryChange(e.target.value)} onBlur={() => setTimeout(() => setShowSuggestions(false), 200)}
              className="absolute inset-0 p-4 bg-transparent text-transparent caret-[var(--color-accent)] font-mono text-[13px] resize-none focus:outline-none leading-relaxed" spellCheck={false} />
            {showSuggestions && suggestions.length > 0 && (
              <div className="absolute bottom-2 left-4 z-10 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-3)] shadow-xl max-h-36 overflow-auto">
                {suggestions.slice(0, 10).map((s) => (
                  <button key={s} onMouseDown={() => applySuggestion(s)} className="block w-full text-left px-3 py-1.5 text-[11px] font-mono text-[var(--color-text)] hover:bg-[var(--color-accent)]/10 hover:text-[var(--color-accent)] cursor-pointer">{s}</button>
                ))}
              </div>
            )}
          </div>

          {/* Validation error */}
          {validation && !validation.valid && validation.error && (
            <div className="px-4 py-2 border-t border-[var(--color-red)]/20 bg-[var(--color-red)]/5 text-[11px] text-[var(--color-red)] font-mono">{validation.error}</div>
          )}

          {/* Action bar */}
          <div className="px-4 py-2.5 border-t border-[var(--color-border)] flex items-center gap-2">
            <button onClick={runExplain} disabled={explainLoading || !query.trim()}
              className="px-3 py-1.5 rounded-md text-[11px] font-semibold cursor-pointer transition-all border border-[var(--color-accent)]/30 bg-[var(--color-accent)]/10 text-[var(--color-accent)] hover:bg-[var(--color-accent)]/20 disabled:opacity-40">
              {explainLoading ? 'Planning...' : 'Explain'}
            </button>
            <button onClick={runLive} disabled={liveLoading || !query.trim()}
              className="px-3 py-1.5 rounded-md text-[11px] font-semibold cursor-pointer transition-all border border-[var(--color-green)]/30 bg-[var(--color-green)]/10 text-[var(--color-green)] hover:bg-[var(--color-green)]/20 disabled:opacity-40">
              {liveLoading ? 'Running...' : 'Run Query'}
            </button>
            {validation?.valid && validation.warnings && validation.warnings.length > 0 && (
              <span className="text-[9px] text-[var(--color-amber)] ml-2">{validation.warnings.length} warning{validation.warnings.length > 1 ? 's' : ''}: {validation.warnings[0]}</span>
            )}
          </div>
        </div>

        {/* ─── Right: Output panels (2 col) ─── */}
        <div className="lg:col-span-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-2)] overflow-hidden">
          {/* Panel tabs */}
          <div className="flex border-b border-[var(--color-border)] bg-[var(--color-surface-3)]">
            {[
              { id: 'transpile' as const, label: 'Transpile' },
              { id: 'explain' as const, label: 'Explain' },
              { id: 'execute' as const, label: 'Execute' },
            ].map((p) => (
              <button key={p.id} onClick={() => setActivePanel(p.id)}
                className={`flex-1 px-3 py-2.5 text-[11px] font-semibold tracking-wider cursor-pointer transition-all ${
                  activePanel === p.id ? 'text-[var(--color-text-bright)] border-b-2 border-[var(--color-accent)]' : 'text-[var(--color-text-dim)] hover:text-[var(--color-text)]'
                }`}>{p.label}</button>
            ))}
          </div>

          <div className="max-h-[500px] overflow-auto">
            {/* Transpile panel */}
            {activePanel === 'transpile' && (
              <div className="divide-y divide-[var(--color-border)]">
                {allBackends.map((b) => {
                  const r = results[b];
                  const isError = r?.startsWith('Error');
                  const meta = backendMeta[b];
                  const isTarget = b === scenario.backend;
                  return (
                    <div key={b} className={`p-3 ${isTarget ? 'bg-[var(--color-surface-3)]' : ''}`}>
                      <div className="flex items-center gap-2 mb-1.5">
                        <span className="w-1.5 h-1.5 rounded-full" style={{ background: meta.color }} />
                        <span className={`text-[10px] font-bold tracking-wider ${isTarget ? 'text-[var(--color-text-bright)]' : 'text-[var(--color-text-dim)]'}`}>{meta.label}</span>
                        <span className="text-[9px] text-[var(--color-text-dim)]">{meta.sub}</span>
                        {isTarget && <span className="text-[8px] px-1.5 py-0.5 rounded-full bg-[var(--color-accent)]/10 text-[var(--color-accent)] ml-auto">TARGET</span>}
                      </div>
                      <pre className={`text-[11px] font-mono leading-relaxed break-all ${isError ? 'text-[var(--color-red)]' : 'text-[var(--color-text)]'}`}>
                        {wasmLoading ? 'Loading WASM...' : r || '--'}
                      </pre>
                    </div>
                  );
                })}
              </div>
            )}

            {/* Explain panel */}
            {activePanel === 'explain' && (
              <div className="p-4">
                {!explainSteps ? (
                  <div className="text-center text-[var(--color-text-dim)] text-[11px] py-8">Click "Explain" to see the execution plan</div>
                ) : explainSteps.length === 0 ? (
                  <div className="text-center text-[var(--color-text-dim)] text-[11px] py-8">No plan available</div>
                ) : (
                  <div className="space-y-2">
                    {explainSteps.map((s) => (
                      <div key={s.step} className="flex items-start gap-2.5">
                        <span className="w-5 h-5 rounded-full bg-[var(--color-surface-3)] border border-[var(--color-border)] flex items-center justify-center text-[9px] font-bold text-[var(--color-text-dim)] shrink-0 mt-0.5">{s.step}</span>
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="text-[11px] text-[var(--color-accent)] font-mono font-semibold">{s.action}</span>
                            {s.backend && <span className="text-[9px] px-1.5 py-0.5 rounded bg-[var(--color-surface-3)] text-[var(--color-text-dim)]">{s.backend}</span>}
                          </div>
                          <div className="text-[10px] text-[var(--color-text-dim)] mt-0.5">{s.detail}</div>
                          {s.native_query && <div className="text-[10px] text-[var(--color-cyan)] font-mono mt-0.5 break-all">{s.native_query}</div>}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            {/* Execute panel */}
            {activePanel === 'execute' && (
              <div className="p-4">
                {!liveResult ? (
                  <div className="text-center text-[var(--color-text-dim)] text-[11px] py-8">Click "Run Query" to execute against live backends</div>
                ) : liveResult.status === 'error' ? (
                  <div>
                    <div className="text-[11px] text-[var(--color-red)] font-semibold mb-1">Error</div>
                    <div className="text-[10px] text-[var(--color-red)] font-mono">{(liveResult as any).error}</div>
                  </div>
                ) : (
                  <div>
                    <div className="flex items-center gap-3 mb-3">
                      <span className="text-[10px] px-2 py-0.5 rounded-full bg-[var(--color-green)]/10 text-[var(--color-green)] font-semibold">SUCCESS</span>
                      <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{liveResult.metadata?.total_time_ms}ms</span>
                      <span className="text-[10px] text-[var(--color-text-dim)] font-mono">{liveResult.metadata?.backend} ({liveResult.metadata?.backend_type})</span>
                    </div>
                    <div className="mb-2">
                      <div className="text-[9px] text-[var(--color-text-dim)] uppercase tracking-wider mb-1">Native Query</div>
                      <div className="text-[10px] text-[var(--color-cyan)] font-mono break-all">{liveResult.metadata?.native_query}</div>
                    </div>
                    <div>
                      <div className="text-[9px] text-[var(--color-text-dim)] uppercase tracking-wider mb-1">Result</div>
                      <pre className="text-[10px] text-[var(--color-text)] font-mono max-h-64 overflow-auto bg-[var(--color-surface)] rounded-md p-2 border border-[var(--color-border)]">
                        {JSON.stringify(liveResult.data, null, 2).slice(0, 3000)}
                      </pre>
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
