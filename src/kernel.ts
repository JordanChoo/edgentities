import init, {
  build_system_prompt,
  build_user_prompt,
  build_continue_prompt,
  parse_response,
  normalize_entity_name,
  dedupe_and_merge,
  list_presets,
  get_preset,
} from "../pkg/extraction_kernel.js";

import wasmModule from "../pkg/extraction_kernel_bg.wasm";

import type { ParseResult, DedupeResult } from "./types.js";

let ready: Promise<unknown> | null = null;

export async function initKernel(): Promise<void> {
  if (!ready) ready = init({ module_or_path: wasmModule });
  await ready;
}

export function buildSystemPrompt(
  entityTypes: string[],
  language: string,
  tupleDelimiter: string,
  completionDelimiter: string,
): string {
  return build_system_prompt(
    JSON.stringify(entityTypes),
    language,
    tupleDelimiter,
    completionDelimiter,
  );
}

export function buildUserPrompt(
  text: string,
  entityTypes: string[],
  language: string,
  completionDelimiter: string,
): string {
  return build_user_prompt(text, JSON.stringify(entityTypes), language, completionDelimiter);
}

export function buildContinuePrompt(
  language: string,
  tupleDelimiter: string,
  completionDelimiter: string,
): string {
  return build_continue_prompt(language, tupleDelimiter, completionDelimiter);
}

export function parseResponse(
  raw: string,
  tupleDelimiter: string,
  completionDelimiter: string,
): ParseResult {
  return parse_response(raw, tupleDelimiter, completionDelimiter) as ParseResult;
}

export function normalizeEntityName(name: string): string {
  return normalize_entity_name(name);
}

export function dedupeAndMerge(
  passes: Array<{
    pass_index: number;
    entities: ParseResult["entities"];
    relationships: ParseResult["relationships"];
  }>,
  mergeDescriptions: boolean,
): DedupeResult {
  return dedupe_and_merge(JSON.stringify(passes), mergeDescriptions) as DedupeResult;
}

export function listPresets(): Record<string, string[]> {
  return JSON.parse(list_presets());
}

export function getPreset(name: string): string[] | null {
  return JSON.parse(get_preset(name));
}
