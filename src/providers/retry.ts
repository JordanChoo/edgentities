import { AppError } from "../errors.js";

const RETRYABLE_STATUS_CODES = new Set([408, 429, 500, 502, 503, 504]);
const BACKOFF_MS = [250, 1000, 4000];

export async function withRetry<T>(
  fn: () => Promise<T>,
  maxRetries: number,
  providerName: string,
): Promise<T> {
  let lastError: Error | null = null;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await fn();
    } catch (err) {
      lastError = err as Error;

      if (attempt >= maxRetries) break;
      if (!isRetryable(err)) break;

      const delay = BACKOFF_MS[Math.min(attempt, BACKOFF_MS.length - 1)];
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }

  throw lastError;
}

function isRetryable(err: unknown): boolean {
  if (err instanceof AppError) {
    return err.retryable;
  }
  if (err instanceof Error && "status" in err) {
    return RETRYABLE_STATUS_CODES.has((err as { status: number }).status);
  }
  return false;
}

export function mapProviderError(status: number, provider: string, model: string): AppError {
  if (status === 401 || status === 403) {
    return new AppError("PROVIDER_AUTH_ERROR", `${provider} rejected credentials`, {
      provider,
      model,
    });
  }
  if (status === 429) {
    return new AppError("PROVIDER_RATE_LIMITED", `${provider} rate limited`, { provider, model });
  }
  return new AppError("PROVIDER_ERROR", `${provider} returned ${status}`, { provider, model });
}
