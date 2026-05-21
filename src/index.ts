import { Hono } from "hono";
import type { Env } from "./types.js";
import { authenticate } from "./auth.js";
import { AppError, serializeError } from "./errors.js";
import { extract } from "./extractor.js";
import { initKernel, listPresets } from "./kernel.js";

type HonoEnv = { Bindings: Env };
const app = new Hono<HonoEnv>();

function getRequestId(request: Request): string {
  return request.headers.get("X-Request-ID") ?? crypto.randomUUID();
}

app.get("/v1/health", (c) => {
  return c.json({ status: "ok" });
});

app.get("/v1/version", (c) => {
  return c.json({
    version: "0.1.0",
    git_sha: "dev",
    built_at: new Date().toISOString(),
  });
});

app.get("/v1/ready", async (c) => {
  const requestId = getRequestId(c.req.raw);
  try {
    authenticate(c.req.raw, c.env);
  } catch (err) {
    if (err instanceof AppError) {
      return c.json(serializeError(err, requestId), err.httpStatus as 401);
    }
    throw err;
  }

  const hasProvider =
    c.env.OPENAI_API_KEY || c.env.ANTHROPIC_API_KEY || c.env.GEMINI_API_KEY;

  if (!hasProvider) {
    return c.json({ status: "unavailable", reason: "No provider keys configured" }, 503);
  }

  return c.json({ status: "ready" });
});

app.get("/v1/presets", async (c) => {
  const requestId = getRequestId(c.req.raw);
  try {
    authenticate(c.req.raw, c.env);
  } catch (err) {
    if (err instanceof AppError) {
      return c.json(serializeError(err, requestId), err.httpStatus as 401);
    }
    throw err;
  }

  await initKernel();
  const presets = listPresets();
  return c.json({ presets });
});

app.post("/v1/extract", async (c) => {
  const requestId = getRequestId(c.req.raw);
  try {
    authenticate(c.req.raw, c.env);
  } catch (err) {
    if (err instanceof AppError) {
      return c.json(serializeError(err, requestId), err.httpStatus as 401);
    }
    throw err;
  }

  try {
    const body = await c.req.json();
    const result = await extract(body, c.env, requestId);
    return c.json(result);
  } catch (err) {
    if (err instanceof AppError) {
      return c.json(serializeError(err, requestId), err.httpStatus as 400);
    }
    const internal = new AppError("INTERNAL_ERROR", "Unexpected error");
    return c.json(serializeError(internal, requestId), 500);
  }
});

app.all("*", (c) => {
  return c.json({ error: { code: "NOT_FOUND", message: "Not found" } }, 404);
});

export default app;
