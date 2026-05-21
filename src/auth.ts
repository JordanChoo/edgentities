import { AppError } from "./errors.js";
import type { Env } from "./types.js";

export function authenticate(request: Request, env: Env): void {
  const keys = parseApiKeys(env.API_KEYS);
  if (keys.length === 0) {
    throw new AppError("INTERNAL_ERROR", "No API keys configured");
  }

  const token = extractToken(request);
  if (!token) {
    throw new AppError("UNAUTHORIZED", "Missing authentication token");
  }

  const valid = keys.some((key) => timingSafeEqual(token, key));
  if (!valid) {
    throw new AppError("UNAUTHORIZED", "Invalid authentication token");
  }
}

function extractToken(request: Request): string | null {
  const authHeader = request.headers.get("Authorization");
  if (authHeader?.startsWith("Bearer ")) {
    return authHeader.slice(7);
  }

  const url = new URL(request.url);
  const csvkey = url.searchParams.get("csvkey");
  if (csvkey) {
    return csvkey;
  }

  return null;
}

function parseApiKeys(raw: string | undefined): string[] {
  if (!raw) return [];
  return raw
    .split(",")
    .map((k) => k.trim())
    .filter((k) => k.length > 0);
}

function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) {
    return false;
  }
  const encoder = new TextEncoder();
  const bufA = encoder.encode(a);
  const bufB = encoder.encode(b);
  let result = 0;
  for (let i = 0; i < bufA.length; i++) {
    result |= bufA[i] ^ bufB[i];
  }
  return result === 0;
}
