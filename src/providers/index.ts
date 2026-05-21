import type { Provider, Env } from "../types.js";
import { AppError } from "../errors.js";
import { OpenAIProvider } from "./openai.js";
import { AnthropicProvider } from "./anthropic.js";
import { GeminiProvider } from "./gemini.js";

export function resolveProvider(env: Env, providerName?: string): Provider {
  const name = providerName ?? env.DEFAULT_PROVIDER ?? "openai";
  const maxRetries = parseInt(env.LLM_MAX_RETRIES ?? "3", 10);

  switch (name) {
    case "openai": {
      if (!env.OPENAI_API_KEY) {
        throw new AppError("INVALID_REQUEST", "OpenAI provider not configured (missing API key)");
      }
      return new OpenAIProvider(env.OPENAI_API_KEY, env.OPENAI_DEFAULT_MODEL, maxRetries);
    }
    case "anthropic": {
      if (!env.ANTHROPIC_API_KEY) {
        throw new AppError(
          "INVALID_REQUEST",
          "Anthropic provider not configured (missing API key)",
        );
      }
      return new AnthropicProvider(env.ANTHROPIC_API_KEY, env.ANTHROPIC_DEFAULT_MODEL, maxRetries);
    }
    case "gemini": {
      if (!env.GEMINI_API_KEY) {
        throw new AppError("INVALID_REQUEST", "Gemini provider not configured (missing API key)");
      }
      return new GeminiProvider(env.GEMINI_API_KEY, env.GEMINI_DEFAULT_MODEL, maxRetries);
    }
    default:
      throw new AppError("INVALID_REQUEST", `Unsupported provider: ${name}`);
  }
}
