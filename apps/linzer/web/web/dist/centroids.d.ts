/* tslint:disable */
/* eslint-disable */
export function annotate(source_url: string): Promise<Annotated>;
export class Annotated {
  private constructor();
  free(): void;
  centroids(): any;
  bounds(): any;
  rays(): any;
  summary_renderer(width: number, height: number): CanvasSummaryRenderer;
}
export class CanvasSummaryRenderer {
  private constructor();
  free(): void;
  render(): Uint8Array;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_canvassummaryrenderer_free: (a: number, b: number) => void;
  readonly canvassummaryrenderer_render: (a: number) => [number, number, number, number];
  readonly __wbg_annotated_free: (a: number, b: number) => void;
  readonly annotated_centroids: (a: number) => any;
  readonly annotated_bounds: (a: number) => any;
  readonly annotated_rays: (a: number) => any;
  readonly annotated_summary_renderer: (a: number, b: number, c: number) => number;
  readonly annotate: (a: number, b: number) => any;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_export_2: WebAssembly.Table;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export_5: WebAssembly.Table;
  readonly __externref_table_dealloc: (a: number) => void;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly closure76_externref_shim: (a: number, b: number, c: any) => void;
  readonly closure211_externref_shim: (a: number, b: number, c: any, d: any) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
