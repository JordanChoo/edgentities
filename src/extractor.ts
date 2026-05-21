import type { Provider, Message, Env } from "./types.js";
import { AppError } from "./errors.js";
import {
  initKernel,
  buildSystemPrompt,
  buildUserPrompt,
  buildContinuePrompt,
  parseResponse,
  dedupeAndMerge,
  getPreset,
} from "./kernel.js";
import { validateExtractRequest, type ExtractRequest } from "./validate.js";
import { resolveProvider } from "./providers/index.js";

export interface ExtractionResponse {
  request_id: string;
  entities: Array<{
    name: string;
    type: string;
    description: string;
    source_pass: number;
  }>;
  relationships: Array<{
    source: string;
    target: string;
    keywords: string[];
    description: string;
    source_pass: number;
  }>;
  stats: {
    input_tokens: number;
    output_tokens: number;
    passes_executed: number;
    complete_signal_received: boolean;
    malformed_lines_dropped: number;
    duration_ms: number;
    provider: string;
    model: string;
    entity_types_resolved: string[];
  };
  warnings: string[];
}

export async function extract(
  body: unknown,
  env: Env,
  requestId: string,
): Promise<ExtractionResponse> {
  await initKernel();

  const maxChars = parseInt(env.MAX_INPUT_CHARS ?? "32000", 10);
  const req = validateExtractRequest(body, maxChars);
  const timeoutMs = parseInt(env.LLM_TIMEOUT_MS ?? "60000", 10);

  const entityTypes = resolveEntityTypes(req);
  const provider = resolveProvider(env, req.provider);

  const start = Date.now();
  let totalInputTokens = 0;
  let totalOutputTokens = 0;
  let totalMalformed = 0;
  let lastComplete = false;
  let model = "";
  const warnings: string[] = [];

  const passes: Array<{
    pass_index: number;
    entities: Array<{ name: string; entity_type: string; description: string }>;
    relationships: Array<{
      source: string;
      target: string;
      keywords: string[];
      description: string;
    }>;
  }> = [];

  const systemPrompt = buildSystemPrompt(
    entityTypes,
    req.language,
    req.tuple_delimiter,
    req.completion_delimiter,
  );
  const userPrompt = buildUserPrompt(
    req.text,
    entityTypes,
    req.language,
    req.completion_delimiter,
  );

  const history: Message[] = [
    { role: "system", content: systemPrompt },
    { role: "user", content: userPrompt },
  ];

  let lastAssistantContent = "";

  for (let passIdx = 0; passIdx <= req.glean_passes; passIdx++) {
    if (passIdx > 0) {
      if (!lastAssistantContent) {
        break;
      }
      history.push({ role: "assistant", content: lastAssistantContent });
      const continuePrompt = buildContinuePrompt(
        req.language,
        req.tuple_delimiter,
        req.completion_delimiter,
      );
      history.push({ role: "user", content: continuePrompt });
    }

    const signal = AbortSignal.timeout(timeoutMs);
    let result;
    try {
      result = await provider.complete({
        messages: [...history],
        model: req.model,
        temperature: req.temperature,
        max_tokens: req.max_output_tokens,
        request_id: requestId,
        signal,
      });
    } catch (err) {
      if (err instanceof Error && err.name === "TimeoutError") {
        throw new AppError("PROVIDER_TIMEOUT", `${provider.name} timed out after ${timeoutMs}ms`, {
          provider: provider.name,
          model: req.model ?? "",
          elapsed_ms: Date.now() - start,
        });
      }
      throw err;
    }

    model = result.model;
    totalInputTokens += result.input_tokens;
    totalOutputTokens += result.output_tokens;
    lastAssistantContent = result.text;

    const parsed = parseResponse(result.text, req.tuple_delimiter, req.completion_delimiter);
    totalMalformed += parsed.malformed_lines_dropped;
    lastComplete = parsed.is_complete;

    passes.push({
      pass_index: passIdx,
      entities: parsed.entities,
      relationships: parsed.relationships,
    });

    if (passIdx > 0 && parsed.entities.length === 0 && parsed.relationships.length === 0) {
      break;
    }
  }

  if (!lastComplete) {
    warnings.push("LLM response did not include completion delimiter; output may be partial");
  }
  if (totalMalformed > 0) {
    warnings.push(`${totalMalformed} malformed lines were dropped during parsing`);
  }

  const merged = dedupeAndMerge(passes, req.merge_descriptions);

  if (merged.entities.length === 0 && merged.relationships.length === 0) {
    warnings.push("No entities or relationships were extracted");
  }

  return {
    request_id: requestId,
    entities: merged.entities.map((e) => ({
      name: e.name,
      type: e.entity_type,
      description: e.description,
      source_pass: e.source_pass,
    })),
    relationships: merged.relationships.map((r) => ({
      source: r.source,
      target: r.target,
      keywords: r.keywords,
      description: r.description,
      source_pass: r.source_pass,
    })),
    stats: {
      input_tokens: totalInputTokens,
      output_tokens: totalOutputTokens,
      passes_executed: passes.length,
      complete_signal_received: lastComplete,
      malformed_lines_dropped: totalMalformed,
      duration_ms: Date.now() - start,
      provider: provider.name,
      model,
      entity_types_resolved: entityTypes,
    },
    warnings,
  };
}

function resolveEntityTypes(req: ExtractRequest): string[] {
  if (req.entity_types) {
    return req.entity_types;
  }
  const presetName = req.preset ?? "general";
  const types = getPreset(presetName);
  if (!types) {
    throw new AppError("INVALID_REQUEST", `Unknown preset: ${presetName}`);
  }
  return types;
}
