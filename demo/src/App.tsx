import { useState, useEffect } from 'react';
import { useWasm } from './hooks/useWasm';

import Header from './components/Header';
import OverviewTab from './components/OverviewTab';
import LiveTab from './components/LiveTab';
import TranspileTab from './components/TranspileTab';
import InvestigateTab from './components/InvestigateTab';
import DocsTab from './components/DocsTab';

export type TabId = 'overview' | 'live' | 'transpile' | 'investigate' | 'docs';

const ENGINE_URL = `http://${window.location.hostname}:9090`;

export interface EngineHealth {
  status: string;
  version: string;
  backends: { name: string; type: string; url: string; status: string }[];
}

export default function App() {
  const { wasm, loading: wasmLoading, transpile } = useWasm();
  const [tab, setTab] = useState<TabId>('overview');
  const [engine, setEngine] = useState<EngineHealth | null>(null);
  const [now, setNow] = useState(new Date());

  useEffect(() => {
    const check = () => {
      fetch(`${ENGINE_URL}/health`)
        .then((r) => r.json())
        .then((d) => setEngine(d))
        .catch(() => setEngine(null));
    };
    check();
    const i = setInterval(check, 30_000);
    return () => clearInterval(i);
  }, []);

  useEffect(() => {
    const i = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(i);
  }, []);

  return (
    <div className="min-h-screen bg-[var(--color-surface)] flex flex-col">
      <Header tab={tab} setTab={setTab} engine={engine} wasmReady={!wasmLoading && !!wasm} now={now} />
      <main className="flex-1 px-4 pb-4 max-w-[1400px] w-full mx-auto">
        {tab === 'overview' && <OverviewTab engine={engine} wasm={wasm} transpile={transpile} setTab={setTab} />}
        {tab === 'live' && <LiveTab />}
        {tab === 'transpile' && <TranspileTab wasm={wasm} wasmLoading={wasmLoading} transpile={transpile} />}
        {tab === 'investigate' && <InvestigateTab engine={engine} />}
        {tab === 'docs' && <DocsTab />}
      </main>
    </div>
  );
}
