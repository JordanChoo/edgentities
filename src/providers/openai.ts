import type { Provider, CompletionOptions, CompletionResult, Message } from "../types.js";
import { withRetry, mapProviderError } from "./retry.js";

export class OpenAIProvider implements Provider {
  name = "openai";
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

    return withRetry(
      async () => {
        const response = await fetch("https://api.openai.com/v1/chat/completions", {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${this.apiKey}`,
            "X-Request-ID": opts.request_id,
          },
          body: JSON.stringify({
            model,
            messages: opts.messages.map(formatMessage),
            temperature: opts.temperature ?? 0,
            ...(opts.max_tokens ? { max_tokens: opts.max_tokens } : {}),
          }),
          signal: opts.signal,
        });

        if (!response.ok) {
          throw mapProviderError(response.status, this.name, model);
        }

        const data = (await response.json()) as OpenAIResponse;
        const choice = data.choices?.[0];

        return {
          text: choice?.message.content ?? "",
          input_tokens: data.usage?.prompt_tokens ?? 0,
          output_tokens: data.usage?.completion_tokens ?? 0,
          model: data.model ?? model,
          finish_reason: mapFinishReason(choice?.finish_reason),
        };
      },
      this.maxRetries,
      this.name,
    );
  }
}

function formatMessage(msg: Message) {
  return { role: msg.role, content: msg.content };
}

function mapFinishReason(reason: string | undefined): CompletionResult["finish_reason"] {
  switch (reason) {
    case "stop":
      return "stop";
    case "length":
      return "length";
    case "content_filter":
      return "content_filter";
    default:
      return "other";
  }
}

interface OpenAIResponse {
  choices?: Array<{
    message: { content: string | null };
    finish_reason: string;
  }>;
  usage?: { prompt_tokens: number; completion_tokens: number };
  model?: string;
}
