/* tslint:disable */
/* eslint-disable */
export function annotate(source_url: string): Promise<AnnotatedJS>;
export class AnnotatedJS {
  private constructor();
  free(): void;
  centroids(): any;
  bounds(): any;
  rays(): any;
  summaries(): any;
  id_of_closest_centroid(x: number, y: number): any;
}
export class Ray {
  private constructor();
  free(): void;
}
export class RegionSummary {
  private constructor();
  free(): void;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_ray_free: (a: number, b: number) => void;
  readonly __wbg_regionsummary_free: (a: number, b: number) => void;
  readonly __wbg_annotatedjs_free: (a: number, b: number) => void;
  readonly annotatedjs_centroids: (a: number) => any;
  readonly annotatedjs_bounds: (a: number) => any;
  readonly annotatedjs_rays: (a: number) => any;
  readonly annotatedjs_summaries: (a: number) => any;
  readonly annotatedjs_id_of_closest_centroid: (a: number, b: number, c: number) => any;
  readonly annotate: (a: number, b: number) => any;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_export_2: WebAssembly.Table;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export_5: WebAssembly.Table;
  readonly closure79_externref_shim: (a: number, b: number, c: any) => void;
  readonly closure214_externref_shim: (a: number, b: number, c: any, d: any) => void;
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
