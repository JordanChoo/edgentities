import type { Provider, CompletionOptions, CompletionResult, Message } from "../types.js";
import { AppError } from "../errors.js";
import { withRetry, mapProviderError } from "./retry.js";

export class AnthropicProvider implements Provider {
  name = "anthropic";
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
    const { systemContent, messages } = splitSystemMessage(opts.messages);

    return withRetry(
      async () => {
        const response = await fetch("https://api.anthropic.com/v1/messages", {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            "x-api-key": this.apiKey,
            "anthropic-version": "2023-06-01",
          },
          body: JSON.stringify({
            model,
            max_tokens: opts.max_tokens ?? 4096,
            temperature: opts.temperature ?? 0,
            ...(systemContent ? { system: systemContent } : {}),
            messages: messages.map((m) => ({ role: m.role, content: m.content })),
          }),
          signal: opts.signal,
        });

        if (!response.ok) {
          throw mapProviderError(response.status, this.name, model);
        }

        const data = (await response.json()) as AnthropicResponse;
        const text = data.content?.[0]?.text ?? "";
        if (!text) {
          throw new AppError("EXTRACTION_EMPTY", "Anthropic returned no content");
        }

        return {
          text,
          input_tokens: data.usage?.input_tokens ?? 0,
          output_tokens: data.usage?.output_tokens ?? 0,
          model: data.model ?? model,
          finish_reason: mapStopReason(data.stop_reason),
        };
      },
      this.maxRetries,
      this.name,
    );
  }
}

function splitSystemMessage(messages: Message[]): {
  systemContent: string | null;
  messages: Message[];
} {
  const systemMsgs = messages.filter((m) => m.role === "system");
  const nonSystemMsgs = messages.filter((m) => m.role !== "system");
  return {
    systemContent: systemMsgs.length > 0 ? systemMsgs.map((m) => m.content).join("\n") : null,
    messages: nonSystemMsgs,
  };
}

function mapStopReason(reason: string | null | undefined): CompletionResult["finish_reason"] {
  switch (reason) {
    case "end_turn":
      return "stop";
    case "max_tokens":
      return "length";
    default:
      return "other";
  }
}

interface AnthropicResponse {
  content?: Array<{ text: string }>;
  usage?: { input_tokens: number; output_tokens: number };
  model?: string;
  stop_reason?: string | null;
}
