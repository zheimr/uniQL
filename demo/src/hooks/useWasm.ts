import { useState, useEffect, useCallback } from 'react';

interface WasmModule {
  parse: (input: string) => string;
  to_logql: (input: string) => string;
  to_logsql: (input: string) => string;
  to_promql: (input: string) => string;
  validate: (input: string) => string;
  explain: (input: string) => string;
  autocomplete: (input: string, cursor: number) => string;
}

interface UseWasmResult {
  wasm: WasmModule | null;
  loading: boolean;
  error: string | null;
  transpile: (query: string, backend: string) => string | null;
}

export function useWasm(): UseWasmResult {
  const [wasm, setWasm] = useState<WasmModule | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function loadWasm() {
      try {
        const wasmUrl = new URL('/wasm/uniql_wasm_bg.wasm', window.location.origin);
        const response = await fetch(wasmUrl);
        const bytes = await response.arrayBuffer();

        const importObject = {
          './uniql_wasm_bg.js': {
            __wbg_Error_83742b46f01ce22d: (arg0: number, arg1: number) => {
              return new Error(getStringFromMemory(arg0, arg1));
            },
            __wbindgen_init_externref_table: () => {
              const table = instance.exports.__wbindgen_externrefs as WebAssembly.Table;
              const offset = table.grow(4);
              table.set(0, undefined);
              table.set(offset + 0, undefined);
              table.set(offset + 1, null);
              table.set(offset + 2, true);
              table.set(offset + 3, false);
            },
          },
        };

        const { instance } = await WebAssembly.instantiate(bytes, importObject);
        const exports = instance.exports as Record<string, unknown>;

        (exports.__wbindgen_start as () => void)();

        let cachedUint8Array: Uint8Array | null = null;
        function getMemory(): Uint8Array {
          if (!cachedUint8Array || cachedUint8Array.byteLength === 0) {
            cachedUint8Array = new Uint8Array(
              (exports.memory as WebAssembly.Memory).buffer
            );
          }
          return cachedUint8Array;
        }

        const decoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        const encoder = new TextEncoder();

        function getStringFromMemory(ptr: number, len: number): string {
          return decoder.decode(getMemory().subarray(ptr >>> 0, (ptr >>> 0) + len));
        }

        let WASM_VECTOR_LEN = 0;
        function passStringToWasm(arg: string): [number, number] {
          const malloc = exports.__wbindgen_malloc as (a: number, b: number) => number;
          const realloc = exports.__wbindgen_realloc as (
            a: number,
            b: number,
            c: number,
            d: number
          ) => number;

          let len = arg.length;
          let ptr = malloc(len, 1) >>> 0;
          const mem = getMemory();
          let offset = 0;

          for (; offset < len; offset++) {
            const code = arg.charCodeAt(offset);
            if (code > 0x7f) break;
            mem[ptr + offset] = code;
          }

          if (offset !== len) {
            if (offset !== 0) arg = arg.slice(offset);
            ptr = realloc(ptr, len, (len = offset + arg.length * 3), 1) >>> 0;
            const view = getMemory().subarray(ptr + offset, ptr + len);
            const ret = encoder.encodeInto(arg, view);
            offset += ret.written!;
            ptr = realloc(ptr, len, offset, 1) >>> 0;
          }

          WASM_VECTOR_LEN = offset;
          return [ptr, WASM_VECTOR_LEN];
        }

        function takeFromExternrefTable(idx: number) {
          const table = exports.__wbindgen_externrefs as WebAssembly.Table;
          const value = table.get(idx);
          (exports.__externref_table_dealloc as (a: number) => void)(idx);
          return value;
        }

        function callWasmFn(
          fn: (a: number, b: number) => [number, number, number, number],
          input: string
        ): string {
          const [ptr, len] = passStringToWasm(input);
          const ret = fn(ptr, len);
          let rPtr = ret[0];
          let rLen = ret[1];
          if (ret[3]) {
            rPtr = 0;
            rLen = 0;
            throw takeFromExternrefTable(ret[2]);
          }
          try {
            return getStringFromMemory(rPtr, rLen);
          } finally {
            (exports.__wbindgen_free as (a: number, b: number, c: number) => void)(
              rPtr,
              rLen,
              1
            );
          }
        }

        type WasmFn = (a: number, b: number) => [number, number, number, number];
        type WasmFn3 = (a: number, b: number, c: number) => [number, number, number, number];

        function callWasmFn2(
          fn: WasmFn3,
          input: string,
          num: number
        ): string {
          const [ptr, len] = passStringToWasm(input);
          const ret = fn(ptr, len, num);
          let rPtr = ret[0];
          let rLen = ret[1];
          if (ret[3]) {
            rPtr = 0;
            rLen = 0;
            throw takeFromExternrefTable(ret[2]);
          }
          try {
            return getStringFromMemory(rPtr, rLen);
          } finally {
            (exports.__wbindgen_free as (a: number, b: number, c: number) => void)(
              rPtr,
              rLen,
              1
            );
          }
        }

        const mod: WasmModule = {
          parse: (input: string) =>
            callWasmFn(exports.parse as WasmFn, input),
          to_logql: (input: string) =>
            callWasmFn(exports.to_logql as WasmFn, input),
          to_logsql: (input: string) =>
            callWasmFn(exports.to_logsql as WasmFn, input),
          to_promql: (input: string) =>
            callWasmFn(exports.to_promql as WasmFn, input),
          validate: (input: string) =>
            callWasmFn(exports.validate as WasmFn, input),
          explain: (input: string) =>
            callWasmFn(exports.explain as WasmFn, input),
          autocomplete: (input: string, cursor: number) =>
            callWasmFn2(exports.autocomplete as WasmFn3, input, cursor),
        };

        if (!cancelled) {
          setWasm(mod);
          setLoading(false);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load WASM');
          setLoading(false);
        }
      }
    }

    loadWasm();
    return () => {
      cancelled = true;
    };
  }, []);

  const transpile = useCallback(
    (query: string, backend: string): string | null => {
      if (!wasm) return null;
      try {
        switch (backend) {
          case 'promql':
            return wasm.to_promql(query);
          case 'logql':
            return wasm.to_logql(query);
          case 'logsql':
            return wasm.to_logsql(query);
          default:
            return wasm.to_promql(query);
        }
      } catch (err) {
        return `Error: ${err instanceof Error ? err.message : String(err)}`;
      }
    },
    [wasm]
  );

  return { wasm, loading, error, transpile };
}
