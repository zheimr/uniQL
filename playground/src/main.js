import * as monaco from 'monaco-editor';
import { UNIQL_LANGUAGE_ID, languageDef, themeData } from './uniql-language.js';
import { EXAMPLES } from './examples.js';

// ── WASM ──
let wasm;
async function loadWasm() {
  wasm = await import('../wasm/uniql_wasm.js');
  await wasm.default();
}

// ── Monaco Setup ──
monaco.languages.register({ id: UNIQL_LANGUAGE_ID });
monaco.languages.setMonarchTokensProvider(UNIQL_LANGUAGE_ID, languageDef);
monaco.editor.defineTheme('uniql-dark', themeData);

// Autocomplete
monaco.languages.registerCompletionItemProvider(UNIQL_LANGUAGE_ID, {
  provideCompletionItems: (model, position) => {
    const word = model.getWordUntilPosition(position);
    const range = {
      startLineNumber: position.lineNumber,
      endLineNumber: position.lineNumber,
      startColumn: word.startColumn,
      endColumn: word.endColumn,
    };

    const keywords = [
      'FROM', 'WHERE', 'WITHIN', 'COMPUTE', 'SHOW', 'CORRELATE ON',
      'GROUP BY', 'HAVING', 'PARSE', 'DEFINE', 'AND', 'OR', 'NOT', 'IN',
      'WITHIN last', 'STARTS WITH', 'CONTAINS', 'MATCHES', 'AS',
    ];

    const signals = ['metrics', 'logs', 'traces', 'events'];
    const backends = ['victoria', 'vlogs', 'prometheus', 'loki', 'tempo'];
    const funcs = [
      'rate(value, 5m)', 'avg(value)', 'sum(value)', 'count()',
      'min(value)', 'max(value)', 'p50(value)', 'p95(value)', 'p99(value)',
      'rate(count, 5m)', 'histogram_quantile(0.99, value)',
    ];
    const shows = ['timeseries', 'table', 'timeline', 'heatmap', 'flamegraph', 'topology', 'count', 'alert'];
    const parses = ['json', 'logfmt', 'pattern', 'regexp'];

    const suggestions = [
      ...keywords.map(k => ({
        label: k, kind: monaco.languages.CompletionItemKind.Keyword,
        insertText: k, range,
      })),
      ...signals.map(s => ({
        label: s, kind: monaco.languages.CompletionItemKind.TypeParameter,
        insertText: s, range,
      })),
      ...backends.map(b => ({
        label: b, kind: monaco.languages.CompletionItemKind.Enum,
        insertText: b, range, detail: 'backend',
      })),
      ...funcs.map(f => ({
        label: f.split('(')[0], kind: monaco.languages.CompletionItemKind.Function,
        insertText: f, range, detail: f,
      })),
      ...shows.map(s => ({
        label: s, kind: monaco.languages.CompletionItemKind.Value,
        insertText: s, range, detail: 'SHOW format',
      })),
      ...parses.map(p => ({
        label: p, kind: monaco.languages.CompletionItemKind.Value,
        insertText: p, range, detail: 'PARSE mode',
      })),
    ];

    return { suggestions };
  },
});

// ── Editor Instance ──
const editor = monaco.editor.create(document.getElementById('editor'), {
  value: EXAMPLES[0].query,
  language: UNIQL_LANGUAGE_ID,
  theme: 'uniql-dark',
  fontSize: 14,
  lineHeight: 24,
  fontFamily: "'JetBrains Mono', 'Fira Code', 'SF Mono', monospace",
  fontLigatures: true,
  minimap: { enabled: false },
  scrollBeyondLastLine: false,
  padding: { top: 16, bottom: 16 },
  renderLineHighlight: 'line',
  cursorBlinking: 'smooth',
  cursorSmoothCaretAnimation: 'on',
  smoothScrolling: true,
  tabSize: 2,
  wordWrap: 'on',
  automaticLayout: true,
  suggest: { showKeywords: true },
});

// ── Transpile Logic ──
const outputEl = document.getElementById('output');
const validationTag = document.getElementById('validation-tag');
const backendLabel = document.getElementById('backend-label');
const backendSelect = document.getElementById('backend');
const parseTimeEl = document.getElementById('parse-time');

function getBackendDisplayName(value) {
  const map = { promql: 'PromQL', logsql: 'LogsQL', logql: 'LogQL' };
  return map[value] || value;
}

function transpile() {
  if (!wasm) {
    outputEl.innerHTML = '<span class="output-error">WASM not loaded yet...</span>';
    return;
  }

  const query = editor.getValue();
  const backend = backendSelect.value;
  backendLabel.textContent = getBackendDisplayName(backend);

  const t0 = performance.now();

  // Validate
  let validationResult;
  try {
    validationResult = JSON.parse(wasm.validate(query));
  } catch (e) {
    validationTag.textContent = 'Error';
    validationTag.className = 'tag tag-error';
    outputEl.innerHTML = `<span class="output-error">${escapeHtml(e.message)}</span>`;
    return;
  }

  if (!validationResult.valid) {
    validationTag.textContent = 'Error';
    validationTag.className = 'tag tag-error';

    let html = `<div class="output-section">
      <div class="output-label">Validation Error</div>
      <span class="output-error">${escapeHtml(validationResult.error)}</span>
    </div>`;

    if (validationResult.hint) {
      html += `<div class="output-section">
        <div class="output-label">Hint</div>
        <span style="color:var(--yellow)">${escapeHtml(validationResult.hint)}</span>
      </div>`;
    }

    outputEl.innerHTML = html;
    return;
  }

  if (validationResult.warnings && validationResult.warnings.length > 0) {
    validationTag.textContent = 'Warning';
    validationTag.className = 'tag tag-warn';
  } else {
    validationTag.textContent = 'Valid';
    validationTag.className = 'tag tag-valid';
  }

  // Transpile
  let native;
  try {
    switch (backend) {
      case 'promql': native = wasm.to_promql(query); break;
      case 'logsql': native = wasm.to_logsql(query); break;
      case 'logql': native = wasm.to_logql(query); break;
      default: native = wasm.to_promql(query);
    }
  } catch (e) {
    outputEl.innerHTML = `<div class="output-section">
      <div class="output-label">Transpile Error</div>
      <span class="output-error">${escapeHtml(e.message)}</span>
    </div>`;
    return;
  }

  // AST
  let ast;
  try { ast = wasm.parse(query); } catch (e) { ast = null; }

  const elapsed = (performance.now() - t0).toFixed(1);
  parseTimeEl.innerHTML = `<div class="dot"></div><span>${elapsed}ms</span>`;

  let html = `<div class="output-section">
    <div class="output-label">Native Query (${getBackendDisplayName(backend)})</div>
    <div class="output-native">${escapeHtml(native)}</div>
  </div>`;

  if (validationResult.signals && validationResult.signals.length) {
    html += `<div class="output-section">
      <div class="output-label">Signals</div>
      <span>${validationResult.signals.join(', ')}</span>
    </div>`;
  }

  if (validationResult.clauses) {
    html += `<div class="output-section">
      <div class="output-label">Clauses</div>
      <span>${escapeHtml(validationResult.clauses)}</span>
    </div>`;
  }

  if (validationResult.warnings && validationResult.warnings.length > 0) {
    html += `<div class="output-section">
      <div class="output-label">Warnings</div>
      <span style="color:var(--yellow)">${validationResult.warnings.map(escapeHtml).join('<br>')}</span>
    </div>`;
  }

  if (ast) {
    html += `<div class="output-section">
      <div class="output-label">AST</div>
      <pre class="output-ast">${escapeHtml(ast)}</pre>
    </div>`;
  }

  outputEl.innerHTML = html;
}

function escapeHtml(s) {
  if (!s) return '';
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// ── Event Handlers ──

// Auto-transpile on change (debounced)
let debounceTimer;
editor.onDidChangeModelContent(() => {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(transpile, 300);
});

// Manual transpile
document.getElementById('run-btn').addEventListener('click', transpile);

// Backend change
backendSelect.addEventListener('change', transpile);

// Keyboard shortcut: Ctrl+Enter
editor.addAction({
  id: 'uniql-transpile',
  label: 'Transpile Query',
  keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
  run: transpile,
});

// ── Examples Drawer ──
const drawer = document.getElementById('examples-drawer');
const examplesBtn = document.getElementById('examples-btn');

drawer.innerHTML = EXAMPLES.map((ex, i) => `
  <div class="example-item" data-index="${i}">
    <div class="example-title">${escapeHtml(ex.title)}</div>
    <div class="example-code">${escapeHtml(ex.query.slice(0, 120))}${ex.query.length > 120 ? '...' : ''}</div>
  </div>
`).join('');

examplesBtn.addEventListener('click', () => {
  drawer.classList.toggle('open');
});

drawer.addEventListener('click', (e) => {
  const item = e.target.closest('.example-item');
  if (item) {
    const idx = parseInt(item.dataset.index);
    const example = EXAMPLES[idx];
    editor.setValue(example.query);
    backendSelect.value = example.backend;
    drawer.classList.remove('open');
    transpile();
  }
});

// Close drawer on outside click
document.addEventListener('click', (e) => {
  if (!drawer.contains(e.target) && e.target !== examplesBtn) {
    drawer.classList.remove('open');
  }
});

// ── Init ──
loadWasm().then(() => {
  transpile();
}).catch(err => {
  outputEl.innerHTML = `<span class="output-error">Failed to load WASM: ${escapeHtml(err.message)}</span>`;
});
