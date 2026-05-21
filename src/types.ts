export interface Env {
  DEFAULT_PROVIDER: string;
  OPENAI_DEFAULT_MODEL: string;
  ANTHROPIC_DEFAULT_MODEL: string;
  GEMINI_DEFAULT_MODEL: string;
  MAX_INPUT_CHARS: string;
  LLM_TIMEOUT_MS: string;
  LLM_MAX_RETRIES: string;
  LOG_LEVEL: string;
  API_KEYS?: string;
  OPENAI_API_KEY?: string;
  ANTHROPIC_API_KEY?: string;
  GEMINI_API_KEY?: string;
}

export interface Message {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface CompletionOptions {
  messages: Message[];
  model?: string;
  temperature?: number;
  max_tokens?: number;
  request_id: string;
  signal: AbortSignal;
}

export interface CompletionResult {
  text: string;
  input_tokens: number;
  output_tokens: number;
  model: string;
  finish_reason: "stop" | "length" | "content_filter" | "other";
}

export interface Provider {
  name: string;
  complete(opts: CompletionOptions): Promise<CompletionResult>;
}

export interface ParseResult {
  entities: Array<{
    name: string;
    entity_type: string;
    description: string;
  }>;
  relationships: Array<{
    source: string;
    target: string;
    keywords: string[];
    description: string;
  }>;
  is_complete: boolean;
  malformed_lines_dropped: number;
}

export interface DedupeResult {
  entities: Array<{
    name: string;
    entity_type: string;
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
}
