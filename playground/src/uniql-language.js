// Monaco language definition for UNIQL
export const UNIQL_LANGUAGE_ID = 'uniql';

export const languageDef = {
  defaultToken: '',
  ignoreCase: true,

  keywords: [
    'FROM', 'WHERE', 'WITHIN', 'COMPUTE', 'SHOW', 'CORRELATE',
    'ON', 'GROUP', 'BY', 'HAVING', 'AS', 'AND', 'OR', 'NOT',
    'IN', 'LAST', 'TO', 'PARSE', 'FILTER', 'DEFINE', 'USE',
    'CONTAINS', 'STARTS', 'WITH', 'MATCHES',
    'TODAY', 'THIS_WEEK',
  ],

  signals: ['metrics', 'logs', 'traces', 'events'],

  backends: ['victoria', 'vlogs', 'prometheus', 'loki', 'tempo', 'elastic', 'clickhouse'],

  showFormats: [
    'timeseries', 'table', 'timeline', 'heatmap',
    'flamegraph', 'count', 'alert', 'topology',
  ],

  functions: [
    'rate', 'irate', 'increase', 'count', 'avg', 'sum', 'min', 'max',
    'p50', 'p75', 'p90', 'p95', 'p99', 'p999',
    'stddev', 'stdvar', 'topk', 'bottomk',
    'histogram_quantile', 'predict_linear',
    'count_over_time', 'avg_over_time', 'sum_over_time',
  ],

  parseModes: ['json', 'logfmt', 'pattern', 'regexp', 'regex'],

  operators: [
    '=', '!=', '>', '<', '>=', '<=', '=~', '!~',
    '+', '-', '*', '/', '%', '|>',
  ],

  tokenizer: {
    root: [
      // Comments
      [/--.*$/, 'comment'],

      // Strings
      [/"([^"\\]|\\.)*"/, 'string'],
      [/'([^'\\]|\\.)*'/, 'string'],

      // Duration literals
      [/\b\d+(?:ms|s|m|h|d|w|y)\b/, 'number.duration'],

      // Numbers
      [/\b\d+(\.\d+)?\b/, 'number'],

      // Pipe arrow
      [/\|>/, 'delimiter.pipe'],

      // Operators
      [/[=!><]=?|=~|!~/, 'operator'],
      [/[+\-*/%]/, 'operator'],

      // Punctuation
      [/[{}()\[\],.:;|]/, 'delimiter'],

      // Identifiers and keywords
      [/[a-zA-Z_]\w*/, {
        cases: {
          '@keywords': 'keyword',
          '@signals': 'type.signal',
          '@backends': 'type.backend',
          '@showFormats': 'type.format',
          '@functions': 'function',
          '@parseModes': 'type.parse',
          '__name__': 'variable.predefined',
          '@default': 'identifier',
        },
      }],
    ],
  },
};

export const themeRules = [
  { token: 'keyword', foreground: 'c792ea', fontStyle: 'bold' },
  { token: 'type.signal', foreground: '82aaff' },
  { token: 'type.backend', foreground: 'f78c6c' },
  { token: 'type.format', foreground: 'c3e88d' },
  { token: 'type.parse', foreground: 'ffcb6b' },
  { token: 'function', foreground: '82aaff' },
  { token: 'string', foreground: 'c3e88d' },
  { token: 'number', foreground: 'f78c6c' },
  { token: 'number.duration', foreground: 'ff5370' },
  { token: 'operator', foreground: '89ddff' },
  { token: 'delimiter', foreground: '89ddff' },
  { token: 'delimiter.pipe', foreground: 'c792ea', fontStyle: 'bold' },
  { token: 'comment', foreground: '546e7a', fontStyle: 'italic' },
  { token: 'identifier', foreground: 'eeffff' },
  { token: 'variable.predefined', foreground: 'ffcb6b', fontStyle: 'italic' },
];

export const themeData = {
  base: 'vs-dark',
  inherit: true,
  rules: themeRules,
  colors: {
    'editor.background': '#0a0a0f',
    'editor.foreground': '#eeffff',
    'editor.lineHighlightBackground': '#1a1a25',
    'editor.selectionBackground': '#3a3a5a55',
    'editorCursor.foreground': '#6c5ce7',
    'editorLineNumber.foreground': '#3a3a4a',
    'editorLineNumber.activeForeground': '#6c5ce7',
    'editor.selectionHighlightBackground': '#6c5ce722',
  },
};
