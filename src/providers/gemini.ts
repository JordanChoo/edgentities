import type { Provider, CompletionOptions, CompletionResult, Message } from "../types.js";
import { AppError } from "../errors.js";
import { withRetry, mapProviderError } from "./retry.js";

export class GeminiProvider implements Provider {
  name = "gemini";
  private apiKey: string;
  private defaultModel: string;
  private maxRetries: number;

  constructor(apiKey: string, defaultModel: string, maxRetries: number) {
    this.apiKey = apiKey;
    this.defaultModel = defaultModel;
    this.maxRetries = maxRetries;
  }

  async complete(opts: CompletionOptions): Promise<CompletionResult> {
    const model = opts.model ?? this.defaultModel;
    const { systemInstruction, contents } = formatMessages(opts.messages);

    return withRetry(
      async () => {
        const url = `https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent?key=${this.apiKey}`;

        const response = await fetch(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            ...(systemInstruction
              ? { systemInstruction: { parts: [{ text: systemInstruction }] } }
              : {}),
            contents,
            generationConfig: {
              temperature: opts.temperature ?? 0,
              ...(opts.max_tokens ? { maxOutputTokens: opts.max_tokens } : {}),
            },
          }),
          signal: opts.signal,
        });

        if (!response.ok) {
          throw mapProviderError(response.status, this.name, model);
        }

        const data = (await response.json()) as GeminiResponse;
        const candidate = data.candidates?.[0];
        const text = candidate?.content?.parts?.[0]?.text ?? "";
        if (!text) {
          throw new AppError("EXTRACTION_EMPTY", "Gemini returned no content");
        }

        return {
          text,
          input_tokens: data.usageMetadata?.promptTokenCount ?? 0,
          output_tokens: data.usageMetadata?.candidatesTokenCount ?? 0,
          model,
          finish_reason: mapFinishReason(candidate?.finishReason),
        };
      },
      this.maxRetries,
      this.name,
    );
  }
}

function formatMessages(messages: Message[]): {
  systemInstruction: string | null;
  contents: GeminiContent[];
} {
  let systemInstruction: string | null = null;
  const contents: GeminiContent[] = [];

  for (const msg of messages) {
    if (msg.role === "system") {
      systemInstruction = msg.content;
    } else {
      contents.push({
        role: msg.role === "assistant" ? "model" : "user",
        parts: [{ text: msg.content }],
      });
    }
  }

  return { systemInstruction, contents };
}

function mapFinishReason(reason?: string): CompletionResult["finish_reason"] {
  switch (reason) {
    case "STOP":
      return "stop";
    case "MAX_TOKENS":
      return "length";
    case "SAFETY":
      return "content_filter";
    default:
      return "other";
  }
}

interface GeminiContent {
  role: string;
  parts: Array<{ text: string }>;
}

interface GeminiResponse {
  candidates?: Array<{
    content?: { parts?: Array<{ text: string }> };
    finishReason?: string;
  }>;
  usageMetadata?: {
    promptTokenCount: number;
    candidatesTokenCount: number;
  };
}
