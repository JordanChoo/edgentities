# PRD — Entity Extraction Service on Cloudflare Workers

**Working title:** `extraction-worker`
**Status:** Draft v1.0 — ready for engineering review (revised: hybrid Rust/WASM kernel from day one)
**Owner:** TBD
**Stakeholders:** Backend platform, ML/RAG, DevOps, Security
**Last updated:** 2026-05-21

---

## 1. Executive Summary

Extract edgequake's LLM-driven entity extraction step from the Rust monolith and reimplement it as a single, stateless Cloudflare Worker exposing a versioned HTTP API. The Worker accepts a text chunk plus configuration, calls a configured LLM provider, parses the tuple-delimited response into structured entities and relationships, normalizes names, and returns JSON. All other pipeline stages (chunking, embedding, graph storage, community detection, query) remain in the existing edgequake binary or are out of scope.

The deliverable is one Worker, one primary endpoint (`POST /v1/extract`), a small constellation of supporting endpoints, full functional parity with the current `edgequake-pipeline` extraction module, and observability/auth suitable for production.

The PRD also specifies a maintenance architecture (§19) that prevents drift between the Worker and the monolith over time. v1.0 ships as a hybrid architecture from day one: a TypeScript Worker shell for HTTP routing, auth, and provider calls, with the contract-bound logic (parser, normalizer, prompts, presets, dedupe) implemented as a shared Rust kernel compiled to WASM via `wasm-pack` — still one Worker, one deployment, one isolate, one source of truth. The contract files are vendored directly from the edgequake repository.

---

## 2. Background & Motivation

### 2.1 What edgequake does today

The `edgequake-pipeline` crate orchestrates document ingestion: chunking → entity extraction → embedding → normalization → graph storage (PostgreSQL + Apache AGE + pgvector). The entity extraction step is implemented in:

- `edgequake/crates/edgequake-pipeline/src/extractor.rs` — orchestration and gleaning loop
- `edgequake/crates/edgequake-pipeline/src/prompts/entity_extraction.rs` — system / user / continue prompts
- `edgequake/crates/edgequake-pipeline/src/prompts/parser.rs` — tuple parser
- `edgequake/crates/edgequake-pipeline/src/prompts/normalizer.rs` — name canonicalization

The unit of work is a single chunk (~1200 tokens). Input is text; output is a set of `(name, type, description)` entity tuples and `(source, target, keywords, description)` relation tuples emitted by the LLM in a line-delimited format with `<|#|>` field separator and `<|COMPLETE|>` sentinel. Gleaning re-runs the LLM with a "what did you miss" prompt for 0–N additional passes.

### 2.2 Why extract it onto a Worker

- **Statelessness.** The extraction unit has no database, no native dependencies, no graph traversal. It's pure compute over text with one outbound HTTP call (two with gleaning).
- **Geographic distribution.** Workers run in 300+ POPs; users near a POP get sub-RTT to the extraction endpoint and only the LLM call traverses the wide-area network.
- **Independent scale.** Document ingestion bursts can saturate the monolith's worker pool; an isolated Worker scales horizontally without back-pressure on the rest of the system.
- **Provider flexibility.** A request-level `provider`/`model` parameter lets callers route specific chunks to specific providers (e.g., Gemini for low-cost bulk, OpenAI for high-fidelity).
- **Decoupling.** Future non-Rust callers (TypeScript SDK, MCP server, third parties) can invoke extraction without linking the Rust crate.

### 2.3 Why a single Worker (not Workflows / Durable Objects)

The extraction unit is naturally single-request: one chunk in, one JSON object out. Multi-chunk orchestration, retry policy across chunks, and document-level state belong to the caller. Cloudflare Workflows or Durable Objects may layer on later for orchestrated batches, but they are explicitly out of scope for v1.

---

## 3. Goals & Non-Goals

### 3.1 Goals

- **G1.** Functional parity with `edgequake-pipeline` entity extraction for the default 9-type general preset and all 6 domain presets.
- **G2.** Single Worker, single primary endpoint, stateless.
- **G3.** Support OpenAI, Anthropic, and Google Gemini as v1 providers; pluggable for additional providers.
- **G4.** P50 latency ≤ LLM call + 50 ms; P99 ≤ LLM call + 200 ms (parsing and normalization budget).
- **G5.** Production-grade observability: structured logs, request IDs, per-provider metrics, error taxonomy.
- **G6.** Authenticated by default; no anonymous access.
- **G7.** Backward-compatible with existing edgequake workspace entity-type lists (any UPPERCASE_UNDERSCORED string, max 50, no validation beyond shape).

### 3.2 Non-Goals (v1)

- Document chunking. Callers send pre-chunked text.
- Embedding generation. A separate service or the existing pipeline handles this.
- Graph storage. The Worker returns JSON; the caller persists.
- Community detection, query, retrieval. Out of scope entirely.
- Streaming responses. v1 is batch JSON; SSE is v1.1.
- Workspace metadata storage. The caller resolves workspace → entity-type list and passes the resolved list per request.
- PDF parsing or any non-text input.
- Multi-chunk orchestration, document-level retry, or distributed coordination.

---

## 4. Users & Personas

- **edgequake core service** — primary caller; the Rust binary's ingestion pipeline replaces its inline extraction with HTTP calls to the Worker.
- **edgequake SDK consumers** — TypeScript/Python/Rust SDKs that today call the monolith's `/api/v1/documents/upload` may grow to call `/v1/extract` directly for client-driven pipelines.
- **MCP clients** — agents that want to extract entities from arbitrary text without the full document upload flow.
- **Platform operators** — DevOps responsible for deployment, secret rotation, monitoring, cost.

---

## 5. Glossary

| Term | Definition |
|---|---|
| Chunk | A text segment, typically 200–2000 tokens, that fits within one LLM context. |
| Entity | A `(name, type, description)` tuple extracted by the LLM. |
| Relationship | A `(source, target, keywords, description)` tuple between two entities. |
| Tuple delimiter | The string separating fields within an output line; default `<\|#\|>`. |
| Completion delimiter | Sentinel string the LLM emits when extraction is done; default `<\|COMPLETE\|>`. |
| Gleaning | Re-running the LLM with a "what did you miss" prompt to improve recall. |
| Pass | A single LLM invocation. Initial extraction is pass 0; gleaning passes are 1, 2, … |
| Preset | A named collection of entity types (e.g. `manufacturing`, `healthcare`). |
| Provider | An LLM vendor (OpenAI, Anthropic, Gemini, …). |
| Normalization | Deterministic transformation of an entity name into its canonical form. |

---

## 6. Architecture

### 6.1 System context

```
┌────────────────────────────────────────────────────────────────┐
│                  CALLER (edgequake / SDK / MCP)                │
│  · chunks documents                                            │
│  · resolves workspace → entity_types                           │
│  · calls /v1/extract per chunk, in parallel                    │
│  · persists results downstream (graph, vector store, …)        │
└────────────────────────────┬───────────────────────────────────┘
                             │ HTTPS, Bearer token
                             ▼
┌────────────────────────────────────────────────────────────────┐
│                    extraction-worker (CF Worker)               │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Router (itty-router or hono)                            │  │
│  │   POST /v1/extract        GET /v1/presets                │  │
│  │   GET  /v1/health         GET /v1/ready                  │  │
│  │   GET  /v1/version                                       │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Request validator (zod) → typed request                 │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Prompt builder (system / user / continue prompts)       │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Provider adapter (OpenAI | Anthropic | Gemini)          │  │
│  │   · fetch() with timeout + retry                         │  │
│  │   · maps to a uniform completion interface               │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Gleaning loop (0–N additional passes)                   │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Tuple parser → entities[], relationships[]              │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Normalizer + dedupe + merge                             │  │
│  └──────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Response serializer (JSON, stats, request_id)           │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────────┬───────────────────────────────────┘
                             │ HTTPS
                             ▼
                ┌────────────────────────────┐
                │  LLM Provider (OpenAI /    │
                │  Anthropic / Gemini)       │
                └────────────────────────────┘
```

### 6.2 Implementation language

**Decision: Hybrid TypeScript shell + Rust/WASM kernel from day one.** The contract-bound logic (parser, normalizer, prompts, presets, dedupe) is implemented in Rust, compiled to WASM via `wasm-pack`, and bundled into a TypeScript Worker shell that handles HTTP routing, auth, provider calls, and observability. This eliminates drift risk by construction — the kernel crate is the single source of truth shared with the edgequake monolith.

Rationale for skipping a pure-TypeScript v1.0: the edgequake contract files (parser, normalizer, prompts) are already implemented and tested in Rust. Reimplementing them in TypeScript only to replace them later with WASM is throwaway work. The WASM cold-start cost (~5–20 ms per isolate) is negligible against LLM call latency (1–10 s), and the bundle size (~200–400 KB WASM + ~50–100 KB TS) is well within the 1 MB Workers limit. The TypeScript shell retains all Workers-native advantages (Wrangler, Miniflare, `fetch()`, Secrets bindings).

See **Appendix F** for the full hybrid architecture reference including repo layout, `wasm-bindgen` interface, and build pipeline.

### 6.3 Components

| Component | Responsibility | Location |
|---|---|---|
| Router | URL → handler dispatch | `src/router.ts` |
| Validator | Schema validation, type coercion | `src/validate.ts` |
| Provider adapter | Uniform completion interface across vendors | `src/providers/*.ts` |
| Extractor | Gleaning loop, pass coordination | `src/extractor.ts` |
| Kernel bridge | Thin TS wrapper importing WASM module | `src/kernel.ts` |
| Auth | Bearer-token validation | `src/auth.ts` |
| Observability | Logging, metrics, tracing | `src/obs.ts` |
| Errors | Error catalog and serializer | `src/errors.ts` |
| **Rust kernel (WASM)** | | |
| Prompt builder | Construct system / user / continue prompts | `kernel/src/prompts.rs` |
| Parser | Tuple → structured data | `kernel/src/parser.rs` |
| Normalizer | Name canonicalization | `kernel/src/normalizer.rs` |
| Dedupe | Entity/relation merging | `kernel/src/dedupe.rs` |
| Presets | Static map of preset name → entity-type list | `kernel/src/presets.rs` |

---

## 7. Functional Requirements

### FR-1. Extract entities and relationships from a single chunk

The Worker MUST accept a `POST /v1/extract` request containing a text chunk and return a JSON response with extracted entities, extracted relationships, and per-request statistics. Behavior MUST be functionally equivalent to `edgequake-pipeline::extractor::extract_entities_from_chunk` for the same input, entity types, language, and gleaning count, modulo LLM provider stochasticity.

### FR-2. Configurable entity types

The Worker MUST accept either:
- An explicit `entity_types` array (1–50 elements, each matching `^[A-Z][A-Z0-9_]{0,63}$`), OR
- A `preset` name (one of: `general`, `manufacturing`, `healthcare`, `legal`, `research`, `finance`), OR
- Neither (defaults to `general`).

`entity_types` and `preset` MUST be mutually exclusive. If both are provided, the Worker MUST return `INVALID_REQUEST`. The resolved type list is interpolated into the system and user prompts verbatim, joined with `", "`.

### FR-3. Configurable language

The Worker MUST accept an optional `language` parameter (free-form string, default `"English"`) which is interpolated into the prompt to instruct the LLM on output language.

### FR-4. Gleaning

The Worker MUST accept an optional `glean_passes` parameter (integer, range 0–3, default 1). After the initial extraction (pass 0), the Worker MUST issue up to `glean_passes` additional LLM calls using the continue-extraction prompt, accumulating entities and relationships across passes.

Each pass MUST be tagged in the response with `source_pass: <int>` so callers can audit recall improvement. If a pass returns no new entities or relationships after deduplication, subsequent passes MUST be skipped (early termination).

### FR-5. Configurable delimiters

The Worker MUST accept optional `tuple_delimiter` (default `<|#|>`) and `completion_delimiter` (default `<|COMPLETE|>`) parameters. Both are interpolated into prompts and used by the parser. Both MUST be non-empty strings and MUST NOT contain newlines.

### FR-6. Provider selection

The Worker MUST accept an optional `provider` parameter and optional `model` parameter. Supported providers in v1: `openai`, `anthropic`, `gemini`. If omitted, the Worker uses environment-configured defaults. If the requested provider is unsupported or its credentials are missing, the Worker MUST return `INVALID_REQUEST` with a clear error message.

### FR-7. Tuple parsing

The parser MUST implement the grammar specified in **Appendix C** exactly. Specifically:
- Split response on `\n`.
- For each line: strip surrounding whitespace, skip if empty, terminate if equal to the completion delimiter.
- Split remaining lines on the tuple delimiter.
- Lines starting with `entity` MUST have exactly 4 fields; otherwise drop the line and increment the `malformed_lines_dropped` counter.
- Lines starting with `relation` MUST have exactly 5 fields; otherwise drop and increment.
- All other lines MUST be dropped (commentary, code fences, blank padding) and counted.

### FR-8. Normalization

Entity names (including relationship `source` and `target` fields) MUST be normalized using the algorithm in **Appendix D**. The normalization output MUST be identical to `edgequake-pipeline::prompts::normalizer::normalize_entity_name` for all inputs.

### FR-9. Deduplication

Within a single response:
- Entities sharing the same normalized name MUST be merged. The `type` of the merged entity is the first non-`OTHER` type encountered, falling back to `OTHER`. The `description` is the longest non-empty description, OR a concatenation joined by `" | "` if `merge_descriptions` is enabled (see FR-15).
- Relationships sharing the same unordered `{source, target}` pair MUST be merged. Keywords are unioned; descriptions are merged per the same policy.

### FR-10. Response shape

Responses MUST follow the schema in **§9.4**. Successful responses MUST include `request_id`, `entities`, `relationships`, and `stats`. Stats MUST include input/output token counts, passes executed, completion signal status, malformed-line count, total duration, provider, and model.

### FR-11. Partial-completion handling

If the LLM response ends without the completion delimiter, the Worker MUST still parse and return whatever valid tuples were emitted, with `stats.complete_signal_received = false` and a `warnings` array entry. This is NOT an error; it MUST return HTTP 200.

### FR-12. Authentication

All `/v1/*` endpoints (except `/v1/health` and `/v1/version`) MUST require a Bearer token in the `Authorization` header. The Worker MUST validate the token against a secret stored in Workers Secrets (`API_KEYS`, a comma-separated set of accepted keys). Failed authentication MUST return HTTP 401 with error code `UNAUTHORIZED`. Optional: Cloudflare Access integration for org-internal callers (see Open Question OQ-2).

### FR-13. Health & readiness

- `GET /v1/health` — returns 200 with `{ "status": "ok" }` if the Worker is responsive. No auth required.
- `GET /v1/ready` — returns 200 only if the default provider's credentials are configured and a 1-token ping to the provider succeeds. Returns 503 otherwise. Auth required.
- `GET /v1/version` — returns Worker version, git SHA, build timestamp. No auth required.

### FR-14. Preset discovery

`GET /v1/presets` MUST return the full list of presets and their resolved entity-type lists. Auth required.

### FR-15. Optional description merging

Request MAY include `merge_descriptions: boolean` (default `false`). When `true`, deduplicated entities and relationships concatenate descriptions with `" | "` separator. When `false`, the longest description wins.

### FR-16. Input limits

- `text` MUST be 1–32,000 characters (configurable via `MAX_INPUT_CHARS` env var). Exceeding returns `CONTENT_TOO_LARGE`.
- Total response size from the LLM is capped at the provider's max-tokens default (typically 4096–8192). Truncated responses are handled per FR-11.

### FR-17. Idempotency

Requests MAY include an `Idempotency-Key` header. If supplied, the Worker MAY cache the response in Workers KV for 5 minutes keyed by `(idempotency_key, sha256(request_body))`. Repeated requests with the same key and body return the cached response. Caching is best-effort and not required for correctness. v1.0 implementation is optional; deferred to v1.1 if scope-pressed.

### FR-18. Request ID propagation

Every request MUST be assigned a `request_id` (UUIDv7). If the caller supplies one in the `X-Request-ID` header, the Worker uses it; otherwise the Worker generates one. The `request_id` MUST appear in: response body, all log lines, and outbound calls to the LLM provider (as a header where supported).

---

## 8. Non-Functional Requirements

### NFR-1. Latency

- P50: ≤ LLM call duration + 50 ms (parse, normalize, serialize)
- P95: ≤ LLM call duration + 100 ms
- P99: ≤ LLM call duration + 200 ms

The LLM call dominates wall-clock time. The Worker's overhead is bounded above by parsing complexity, which is O(n) in the number of output tokens.

### NFR-2. Throughput

Per-Worker concurrency target: 100 in-flight requests per isolate, bounded only by Cloudflare's subrequest budget (50/free, 1000/paid). The Worker MUST NOT serialize requests internally.

### NFR-3. CPU time

Total Worker CPU time per request MUST stay under 30 s (free plan) or 300 s (paid). LLM calls are I/O and do not count against CPU; budget is therefore consumed entirely by parsing, JSON serialization, and validation. Expected: < 100 ms CPU per request.

### NFR-4. Subrequest budget

Each request consumes `1 + glean_passes` subrequests for LLM calls. With `glean_passes = 1` (default), each request uses 2 subrequests. The Worker MUST reject requests where the configured plan budget would be exceeded (with `INTERNAL_ERROR`), though in practice the per-request budget is well within both tiers.

### NFR-5. Memory

Worker isolates have a 128 MB ceiling. Peak memory per request is dominated by the LLM response string (up to ~32 KB for typical chunks). Well within budget.

### NFR-6. Reliability

- Worker MUST retry LLM calls on transient errors (HTTP 408, 429, 500, 502, 503, 504) with exponential backoff: 250 ms, 1 s, 4 s, max 3 retries.
- Worker MUST NOT retry on 4xx (other than 429) — these indicate request errors.
- Total retry budget MUST stay under the wall-clock limit; if total elapsed exceeds 90% of the wall-clock budget, no further retries.

### NFR-7. Security

- All secrets stored in Workers Secrets, never in source.
- API keys hashed at rest? No — Workers Secrets are encrypted at rest by Cloudflare; plaintext comparison is acceptable. Use constant-time string comparison to prevent timing attacks.
- TLS enforced by Workers platform; no plaintext.
- No PII in logs. Request text MAY be logged at DEBUG level only, with a documented retention of < 24 h; default log level in production is INFO.
- Rate limiting via Cloudflare Rate Limiting rules per API key, configured outside the Worker. Default: 100 RPS per key, burst 200.

### NFR-8. Observability

- **Logs:** structured JSON, one line per request, including `request_id`, `provider`, `model`, `passes_executed`, `entities_count`, `relationships_count`, `duration_ms`, `malformed_lines_dropped`, `error_code` (if applicable).
- **Metrics:** exported via Workers Analytics Engine. Required series:
  - `requests_total{status, provider, model}`
  - `request_duration_ms{quantile, provider}`
  - `llm_call_duration_ms{quantile, provider, model}`
  - `entities_extracted_total{provider}`
  - `relationships_extracted_total{provider}`
  - `malformed_lines_total{provider}`
  - `provider_errors_total{provider, code}`
- **Tracing:** if Cloudflare Workers Trace Events are available, emit one span per request with attributes for provider, model, and pass count.

### NFR-9. Cost

Estimated per-request cost (paid plan):
- Worker invocation: ~$0.0000003 (negligible)
- Subrequests (2 with default gleaning): negligible
- LLM tokens: dominant cost; passed through to caller via response stats. Caller MAY use stats to track spend.

### NFR-10. Compatibility

Response JSON schema MUST be stable within a major version. Breaking changes require bumping `/v1/` → `/v2/`. Additive changes (new optional fields) are allowed within a major version.

---

## 9. API Specification

### 9.1 Base URL

Production: `https://extraction.<your-zone>.workers.dev` (or custom domain).
Staging: `https://extraction-staging.<your-zone>.workers.dev`.

### 9.2 Authentication

All authenticated endpoints accept a Bearer token:

```
Authorization: Bearer <token>
```

Tokens are pre-shared keys configured via Workers Secrets. Rotation procedure documented in **§13.3**.

### 9.3 Endpoints

| Method | Path | Auth | Description |
|---|---|---|---|
| POST | `/v1/extract` | Yes | Extract entities/relationships from a chunk |
| GET | `/v1/presets` | Yes | List available presets |
| GET | `/v1/health` | No | Liveness probe |
| GET | `/v1/ready` | Yes | Readiness probe (validates provider connectivity) |
| GET | `/v1/version` | No | Worker version metadata |

### 9.4 Request: `POST /v1/extract`

**Headers**

```
Authorization: Bearer <token>           (required)
Content-Type: application/json          (required)
X-Request-ID: <uuidv7>                  (optional)
Idempotency-Key: <opaque>               (optional; v1.1)
```

**Body**

```json
{
  "text": "string (required, 1–32000 chars)",
  "entity_types": ["PERSON", "ORGANIZATION"],
  "preset": "general | manufacturing | healthcare | legal | research | finance",
  "language": "English",
  "glean_passes": 1,
  "provider": "openai | anthropic | gemini",
  "model": "gpt-4o-mini",
  "tuple_delimiter": "<|#|>",
  "completion_delimiter": "<|COMPLETE|>",
  "merge_descriptions": false,
  "temperature": 0.0,
  "max_output_tokens": 4096
}
```

All fields except `text` are optional. `entity_types` and `preset` are mutually exclusive. Defaults are documented per field in §7.

### 9.5 Response: `POST /v1/extract` (200 OK)

```json
{
  "request_id": "01928f47-7e5e-7c4b-9b0a-9b0a9b0a9b0a",
  "entities": [
    {
      "name": "SARAH_CHEN",
      "type": "PERSON",
      "description": "Dr. Sarah Chen is the lead researcher at Quantum Dynamics Lab...",
      "source_pass": 0
    }
  ],
  "relationships": [
    {
      "source": "SARAH_CHEN",
      "target": "QUANTUM_DYNAMICS_LAB",
      "keywords": ["employment", "research"],
      "description": "Sarah Chen works as lead researcher at Quantum Dynamics Lab.",
      "source_pass": 0
    }
  ],
  "stats": {
    "input_tokens": 1243,
    "output_tokens": 487,
    "passes_executed": 2,
    "complete_signal_received": true,
    "malformed_lines_dropped": 0,
    "duration_ms": 4231,
    "provider": "openai",
    "model": "gpt-4o-mini",
    "entity_types_resolved": ["PERSON", "ORGANIZATION", "LOCATION", "EVENT", "CONCEPT", "TECHNOLOGY", "PRODUCT", "OTHER"]
  },
  "warnings": []
}
```

### 9.6 Error response

```json
{
  "error": {
    "code": "PROVIDER_TIMEOUT",
    "message": "Provider openai did not respond within 60s",
    "retryable": true,
    "details": {
      "provider": "openai",
      "model": "gpt-4o-mini",
      "elapsed_ms": 60001
    }
  },
  "request_id": "01928f47-7e5e-7c4b-9b0a-9b0a9b0a9b0a"
}
```

### 9.7 Error catalog

| Code | HTTP | Retryable | Description |
|---|---|---|---|
| `INVALID_REQUEST` | 400 | No | Schema validation failure |
| `UNAUTHORIZED` | 401 | No | Missing or invalid bearer token |
| `FORBIDDEN` | 403 | No | Authenticated but not authorized for resource |
| `CONTENT_TOO_LARGE` | 413 | No | `text` exceeds `MAX_INPUT_CHARS` |
| `RATE_LIMITED` | 429 | Yes | Caller exceeded their RPS quota |
| `PROVIDER_RATE_LIMITED` | 429 | Yes | Upstream LLM provider rate-limited us |
| `PROVIDER_AUTH_ERROR` | 502 | No | Provider rejected credentials (operator action required) |
| `PROVIDER_ERROR` | 502 | Yes | Provider returned 5xx |
| `PROVIDER_TIMEOUT` | 504 | Yes | Provider exceeded request timeout |
| `EXTRACTION_EMPTY` | 422 | No | Provider returned no parseable tuples |
| `INTERNAL_ERROR` | 500 | Yes | Unexpected Worker error |

### 9.8 Request: `GET /v1/presets`

**Response (200 OK)**

```json
{
  "presets": {
    "general": ["PERSON", "ORGANIZATION", "LOCATION", "EVENT", "CONCEPT", "TECHNOLOGY", "PRODUCT", "OTHER"],
    "manufacturing": ["..."],
    "healthcare": ["..."],
    "legal": ["..."],
    "research": ["..."],
    "finance": ["..."]
  }
}
```

Full preset contents in **Appendix B**.

### 9.9 OpenAPI 3.1 spec

A machine-readable OpenAPI spec MUST be published at `/v1/openapi.json` and committed to the repo at `openapi.yaml`. Engineering MUST keep these in sync with the implementation; CI enforces this.

---

## 10. Domain Logic Specification

### 10.1 Prompt construction

Three prompts are constructed per request:

- **System prompt** — instructions, format requirements, entity-type list, few-shot examples. Built once per pass.
- **User prompt** — the chunk text plus a small task reminder. Built for pass 0 only.
- **Continue prompt** — used for gleaning passes (1+) to ask the LLM what it missed.

Verbatim templates with placeholders are in **Appendix A**. Implementations MUST produce byte-identical output to the Rust reference for the same input parameters, modulo whitespace differences that would not affect LLM parsing.

### 10.2 Conversation construction per pass

- **Pass 0:** `[system, user]`
- **Pass N (N ≥ 1):** `[system, user, assistant=<pass 0 output>, user=<continue prompt>, assistant=<pass 1 output>, user=<continue prompt>, ...]` — i.e., full conversation history preserved across passes.

### 10.3 Tuple parsing

Specified formally in **Appendix C**. Key invariants:
- Parsing is line-oriented and order-independent.
- Malformed lines are silently dropped (not error) but counted in `stats.malformed_lines_dropped`.
- The completion delimiter terminates parsing; content after it is discarded.
- Parsing is robust to: surrounding whitespace, blank lines, code fences (```), commentary lines, partial truncation.

### 10.4 Normalization

Specified formally in **Appendix D**. Applied to: every entity `name`, every relationship `source` and `target`. NOT applied to: `type`, `description`, `keywords`.

### 10.5 Dedupe & merge

Operates over the union of entities and relationships from all passes:

1. **Entity dedupe:** group by normalized `name`. For each group:
   - `type`: first non-`OTHER` value, else `OTHER`.
   - `description`: longest non-empty value (or concatenated with `" | "` if `merge_descriptions=true`).
   - `source_pass`: minimum pass number.

2. **Relationship dedupe:** group by `{normalize(source), normalize(target)}` as an unordered set (the prompt declares relationships undirected). For each group:
   - `keywords`: union, deduplicated, sorted.
   - `description`: same policy as entities.
   - `source_pass`: minimum.

### 10.6 Gleaning loop

```pseudo
entities, relationships = []
history = [system_message, user_message]
for pass_idx in 0..=glean_passes:
    if pass_idx > 0:
        history.append(assistant_message(last_response))
        history.append(user_message(continue_prompt))

    response = provider.complete(history, temperature=0.0)
    parsed = parse(response)
    new_entities, new_relationships = dedupe_against(parsed, entities, relationships)

    if pass_idx > 0 and new_entities.is_empty() and new_relationships.is_empty():
        break  // early termination per FR-4

    entities.extend(new_entities)
    relationships.extend(new_relationships)
    last_response = response

return entities, relationships
```

---

## 11. Provider Abstraction

### 11.1 Uniform interface

```typescript
interface Provider {
  name: string;
  complete(opts: CompletionOptions): Promise<CompletionResult>;
}

interface CompletionOptions {
  messages: Message[];
  model?: string;
  temperature?: number;
  max_tokens?: number;
  request_id: string;
  signal: AbortSignal;
}

interface CompletionResult {
  text: string;
  input_tokens: number;
  output_tokens: number;
  model: string;  // actual model used
  finish_reason: "stop" | "length" | "content_filter" | "other";
}
```

### 11.2 Provider implementations

Each provider lives in `src/providers/<name>.ts` and implements the `Provider` interface. v1 ships:

- **OpenAI** — Chat Completions API, `gpt-4o-mini` default.
- **Anthropic** — Messages API, `claude-sonnet-4-6` default.
- **Gemini** — Generative Language API, `gemini-2.5-flash` default.

### 11.3 Provider default routing

If the request omits `provider`, the Worker reads `DEFAULT_PROVIDER` from env (default: `openai`). If the request omits `model`, the Worker uses the provider's documented default. Operators MAY override per-provider defaults via env vars (e.g., `OPENAI_DEFAULT_MODEL`).

### 11.4 Adding a new provider

Process documented in `docs/adding-a-provider.md`. Requires: an adapter file in `src/providers/`, a credential secret, a unit test against a recorded response fixture, and a smoke test invoking the real provider with a known prompt.

---

## 12. Configuration & Secrets

### 12.1 Environment variables (non-secret, in `wrangler.toml`)

| Variable | Default | Purpose |
|---|---|---|
| `DEFAULT_PROVIDER` | `openai` | Provider used when request omits one |
| `OPENAI_DEFAULT_MODEL` | `gpt-4o-mini` | |
| `ANTHROPIC_DEFAULT_MODEL` | `claude-sonnet-4-6` | |
| `GEMINI_DEFAULT_MODEL` | `gemini-2.5-flash` | |
| `MAX_INPUT_CHARS` | `32000` | Per-request input cap |
| `LLM_TIMEOUT_MS` | `60000` | Per-call provider timeout |
| `LLM_MAX_RETRIES` | `3` | Per-call retry count |
| `LOG_LEVEL` | `info` | One of `debug`, `info`, `warn`, `error` |

### 12.2 Secrets (via `wrangler secret put`)

| Secret | Required For |
|---|---|
| `API_KEYS` | All authenticated endpoints (comma-separated accepted keys) |
| `OPENAI_API_KEY` | OpenAI provider |
| `ANTHROPIC_API_KEY` | Anthropic provider |
| `GEMINI_API_KEY` | Google Gemini provider |

### 12.3 KV / R2 / D1 bindings (v1.1+)

For idempotency caching (FR-17), bind a KV namespace named `IDEMPOTENCY`. Not required in v1.0.

---

## 13. Deployment & Infrastructure

### 13.1 Tooling

- **Wrangler** (latest) — local dev, deploys.
- **Miniflare** or `wrangler dev` — local emulation.
- **GitHub Actions** — CI/CD.

### 13.2 Environments

| Env | URL | Purpose |
|---|---|---|
| `dev` | `*.workers.dev` per-developer | Local + ephemeral cloud previews |
| `staging` | `extraction-staging.<zone>` | Pre-prod validation |
| `prod` | `extraction.<zone>` | Live |

### 13.3 Secret rotation

- API keys: monthly rotation. The `API_KEYS` secret holds a comma-separated set, so rotation is non-breaking: add new key → roll callers → remove old key.
- Provider keys: rotated when the provider expires them or per quarterly schedule, whichever is sooner.
- Runbook: `docs/runbooks/rotate-secrets.md`.

### 13.4 Deployment process

1. PR opened → CI runs lint, typecheck, unit tests, contract tests against mock provider, integration tests against real provider in dev project.
2. Merge to `main` → auto-deploy to staging.
3. Manual promotion to prod via GitHub Action `deploy-prod` (requires approval from two reviewers).
4. Post-deploy smoke test runs against prod within 30s; failure triggers automatic rollback to previous version.

### 13.5 Rollback

`wrangler rollback` to previous version. Worker version IDs visible in Cloudflare dashboard. Versions kept for 30 days.

---

## 14. Testing Strategy

### 14.1 Unit tests

**Rust kernel (cargo test):**
- Parser: ≥ 30 cases including malformed lines, mixed delimiters, partial completion, code fences in output, empty response, completion delimiter without prior tuples.
- Normalizer: ≥ 20 cases covering case normalization, punctuation, multilingual input, Unicode edge cases, empty/whitespace-only.
- Prompt builder: snapshot tests ensuring byte-identical output to edgequake reference.
- Dedupe: cases for same-name conflicts, undirected relationship symmetry, description-merge policies.

**TypeScript shell (Vitest):**
- Provider adapters: mock HTTP responses, verify correct request shaping per provider API.
- Auth: valid/invalid/missing token permutations.
- Router: correct dispatch, 404 handling, CORS.
- Kernel bridge: verify WASM loads and exports are callable.

### 14.2 Contract tests

For each provider, a recorded request/response pair (using `msw` or fetch mocks) verifies that the adapter handles the provider's wire format correctly. Updated when provider APIs change.

### 14.3 Parity tests

A test harness that runs the same input through both the Rust pipeline (`edgequake-pipeline` extraction) and the new Worker, using a deterministic temperature=0 provider, asserts that the parsed `entities` and `relationships` sets are equal modulo provider stochasticity. Run in CI nightly against a fixture corpus of ≥ 100 chunks across the 6 presets.

### 14.4 Integration tests

Smoke tests against real providers with a small chunk, run on merge to `main` and nightly. One test per provider per default model. Failures alert on-call.

### 14.5 Load tests

Pre-launch: 1000 RPS sustained for 10 minutes against staging, verifying P99 latency, error rate, and subrequest budget headroom. Tooling: `k6` or `oha`.

### 14.6 Acceptance test suite

Reference `acceptance/` folder containing:
- 50 hand-curated chunks across the 6 presets
- Expected entity sets (allowing ±10% recall variance)
- Expected relationship counts

Run on every release. Manual review of any chunk where Worker output diverges from the expected set by > 10%.

---

## 15. Migration & Rollout Plan

### Phase 0 — Build (weeks 1–3)

- Implement Worker, providers, parser, normalizer per this PRD.
- Unit + contract tests green.
- Deploy to staging.

### Phase 1 — Shadow (week 4)

- edgequake's existing extractor runs as today, but ALSO calls the Worker in parallel (fire-and-forget).
- Both outputs logged. Offline analysis compares parity over 1 week of production traffic.
- Success criterion: ≥ 95% entity-set agreement, ≥ 90% relationship-set agreement, across all presets and providers.

### Phase 2 — Canary (week 5)

- Feature flag in edgequake routes 1% → 10% → 50% of extraction calls to the Worker over 5 days.
- Monitor error rate, latency, entity-count distributions, downstream graph quality metrics.

### Phase 3 — Full cutover (week 6)

- 100% of extraction traffic via Worker.
- Old inline extractor remains in codebase but unused, behind a fallback flag for emergency reversion.
- After 30 days of stable operation, the inline extractor code is removed (PR title: "remove vestigial inline extractor; long live extraction-worker").

### Phase 4 — Hardening (weeks 7+)

- v1.1 features: idempotency cache, SSE streaming, batch endpoint.
- Performance tuning based on production telemetry.

### Phase 5 — Hardening continued (v1.1+)

- Additional provider models benchmarked for extraction quality.
- Per-preset few-shot examples (OQ-8).
- Contract corpus expanded based on production edge cases.

---

## 16. Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| LLM provider rate-limits during peak | M | H | Per-request retry with backoff; expose `provider` to caller for load-shifting; document expected RPS per provider tier |
| Worker subrequest budget exhausted | L | H | Default `glean_passes=1` keeps it at 2/req; document budget in docs; alert at 80% utilization |
| Parser divergence from Rust reference | L | M | Eliminated by shared Rust kernel; parity tests in CI as safety net |
| Provider response format changes | L | H | Contract tests run nightly; alerts on adapter test failures |
| API key leakage | L | H | Workers Secrets; no logging of headers; rotation runbook; rate limiting per key |
| Cost overruns from gleaning loops | L | M | `glean_passes` bounded 0–3; cost stats in response; cost dashboards |
| WASM cold-start overhead | L | L | ~5–20 ms per isolate, negligible vs LLM call latency; amortized to near-zero under load |
| Cold-start latency on rare regions | L | L | Worker isolates warm quickly; expected p99 cold-start < 100 ms (negligible vs LLM call) |

---

## 17. Open Questions

| ID | Question | Owner | Default if unresolved |
|---|---|---|---|
| OQ-1 | Should v1 ship with the batch endpoint (multiple chunks per request) or defer to v1.1? | Product | Defer |
| OQ-2 | Use Cloudflare Access (SSO) for org-internal callers in addition to Bearer tokens? | Security | No, Bearer only in v1 |
| OQ-3 | Should the Worker enforce a per-key spending cap based on response stats? | Product / Finance | No, defer to upstream billing |
| OQ-4 | Should the Worker accept `temperature` and `max_output_tokens` from the request, or fix them to deterministic values? | ML | Accept, defaults `0.0` and `4096` |
| OQ-5 | Should we expose the raw LLM response in the response payload (behind a flag) for debugging? | Eng | Yes, behind `debug=true` query param, only with auth |
| OQ-6 | Should there be a "dry run" mode that returns the constructed prompt without calling the LLM? | Eng | Yes, `?dry_run=true` returns prompt only, no LLM cost |
| OQ-7 | When `entity_types` is omitted, default to `general` preset — or to the legacy 9 default types from edgequake? | Backwards-compat | Whichever matches the Rust default; verify by inspection |
| OQ-8 | Patch `get_examples()` per-preset, or keep the fixed examples? | ML | Defer to v1.1; flag as known limitation in docs |

---

## 18. Acceptance Criteria

For v1.0 to ship, ALL of the following MUST be true:

- [ ] All FR-1 through FR-18 implemented and covered by tests.
- [ ] All NFR-1 through NFR-10 verified via load tests and observability dashboards.
- [ ] OpenAPI 3.1 spec published and CI-enforced against implementation.
- [ ] All 6 presets defined in Appendix B exposed via `/v1/presets`.
- [ ] Verbatim prompt parity with the Rust reference (Appendix A) verified by snapshot tests.
- [ ] Parser conforms to grammar in Appendix C, verified by ≥ 30 unit tests including malformed inputs.
- [ ] Normalizer output equals Rust reference for ≥ 100 fixture inputs.
- [ ] Parity test (§14.3) achieves ≥ 95% entity-set agreement across the 6 presets.
- [ ] All 3 v1 providers (OpenAI, Anthropic, Gemini) pass smoke tests.
- [ ] Auth: requests without `Authorization` header receive 401; with invalid key receive 401; with valid key receive 2xx/4xx based on request.
- [ ] Observability dashboards live in Cloudflare Analytics with the metrics listed in NFR-8.
- [ ] Runbooks published for: secret rotation, rollback, provider outage, parser regression.
- [ ] Staging deployment stable for ≥ 7 days with zero unexplained 5xx.
- [ ] Shadow phase (§15 Phase 1) meets parity success criteria.
- [ ] Security review completed; pen-test report addresses or accepts all findings.
- [ ] Cost projection reviewed and approved by Finance.
- [ ] Maintenance acceptance criteria from §19.7 satisfied (shared `contract/` directory, parity corpus in CI, `contract_version` in responses, upstream-watch GitHub Action, `RELEASE-PARITY.md`, production-sampling mechanism ready).

---

## 19. Maintenance & Evolution

### 19.1 Drift surfaces

Entity extraction will continue to evolve in edgequake — prompt tweaks for recall, new presets for new domains, parser tolerances for new providers, normalizer rules for new locales. Because v1.0 ships with the shared Rust kernel from day one, drift in contract-bound logic (parser, normalizer, prompts, presets, dedupe) is eliminated by construction. This section specifies the remaining maintenance layers: contract files, versioning, testing, and process.

The six surfaces where drift can creep in, ranked by historical change frequency:

1. **Prompt strings** (system, user, continue) — highest churn; recall-focused tweaks land routinely.
2. **Few-shot examples** — currently fixed; expected to grow per-preset (see OQ-8).
3. **Preset definitions** — entity-type lists per domain; expand as new domains land.
4. **Parser tolerances** — handling of malformed lines, new provider output quirks.
5. **Normalizer rules** — Unicode edge cases, locale handling.
6. **Gleaning strategy** — currently linear; future may become parallel or adaptive.

The layers below cover these surfaces. **All layers (19.2–19.6) are required for v1.0.** The shared Rust kernel (Layer 4) eliminates drift in surfaces 1–5 by construction; Layers 1–3 provide versioning, testing, and auditability; Layer 5 covers process.

### 19.2 Layer 1 — Shared contract files (v1.0, required)

Prompts and presets MUST live as data files in the edgequake repository under `contract/`, not as inline format strings in `entity_extraction.rs`:

```
contract/
├── VERSION                          semver, e.g. "1.0.0"
├── prompts/
│   ├── system.tmpl                  with {{entity_types}}, {{language}}, etc.
│   ├── user.tmpl
│   ├── continue.tmpl
│   └── examples.tmpl                few-shot block
└── presets/
    ├── general.json
    ├── manufacturing.json
    ├── healthcare.json
    ├── legal.json
    ├── research.json
    └── finance.json
```

Template placeholders use `{{name}}` mustache-style syntax — pure string substitution, no logic, no conditionals.

Both consumers read these same files:

- **Rust monolith:** `include_str!("../../contract/prompts/system.tmpl")` at compile time; render with `minijinja` or a hand-rolled 30-line substituter.
- **TypeScript Worker:** vendored via git submodule, an npm package published from the edgequake repo, or a `scripts/sync-contract.sh` invoked in CI. Loaded as text via Wrangler's text module loader at build time.

CI in the Worker repo MUST verify the vendored contract version matches the upstream tag it's pinned to; mismatches block merge.

This layer alone eliminates the largest drift category (prompts and presets) by construction. Every prompt change in edgequake flows to the Worker through one diff.

### 19.3 Layer 2 — Parity corpus and golden tests (v1.0, required)

A `contract/parity-corpus/` directory containing ≥ 100 triples of frozen test data:

```
parity-corpus/
└── 001-research-author.json
    {
      "input": "Dr. Sarah Chen, lead researcher at...",
      "frozen_llm_response": "entity<|#|>Sarah Chen<|#|>...\n...<|COMPLETE|>",
      "expected_parsed": { "entities": [...], "relationships": [...] }
    }
```

The `frozen_llm_response` is captured once (from a temperature=0 run against a fixed model) and committed. Both implementations parse `frozen_llm_response` and assert byte-equality against `expected_parsed`. The corpus covers all 6 presets, malformed-line edge cases, partial completions, multi-pass dedupe scenarios, and Unicode normalization corners.

Both repos run the corpus in CI on every PR. The corpus is the source of truth for parser, normalizer, and dedupe behavior; new behavior requires new cases approved by the ML lead. The existing `parser.rs` and `normalizer.rs` unit-test cases MUST be promoted into the corpus on day one.

Failure modes the corpus catches:

- Parser regressions in either implementation
- Normalizer regressions (including Unicode handling)
- Dedupe-policy drift
- Whitespace handling changes

What it does NOT catch (and is not intended to): provider stochasticity, prompt-recall regressions, LLM model changes. Those are caught by integration tests (§14.4) and production sampling (§19.6).

### 19.4 Layer 3 — Contract versioning surfaced in responses (v1.0, required)

The `contract/VERSION` file holds a semver string. The version is:

- Read at build time by both consumers.
- Embedded as `stats.contract_version` in every Worker response (additive to the schema in §9.5).
- Embedded in Worker log lines.
- Bumped on every change to any file under `contract/`. CI in the edgequake repo enforces this: a PR touching `contract/**` without bumping `VERSION` fails the `contract-version-bump` check.

Semver rules:

- **Patch:** wording tweaks, typo fixes, additive few-shot examples, new preset added.
- **Minor:** new optional fields in parsed output, additive parser tolerances, new tuple-delimiter support.
- **Major:** breaking changes to prompt structure, parser grammar, or normalizer output. Requires API version bump (`/v1/` → `/v2/`).

Callers MAY pin to a major version and alert on minor drift via their own observability.

### 19.5 Layer 4 — Shared Rust kernel via WASM (v1.0, shipping from day one)

The contract-bound logic is shared as compiled code from v1.0. The `edgequake-extraction-core` crate is the single source of truth:

```
edgequake-extraction-core/        ← new Rust crate, in the edgequake repo
├── Cargo.toml
└── src/
    ├── lib.rs                    public API
    ├── prompts.rs                template loading and rendering
    ├── parser.rs                 tuple parsing
    ├── normalizer.rs             name canonicalization
    ├── dedupe.rs                 entity/relation merging
    └── presets.rs                preset definitions
```

Two consumers, one source of truth:

- **edgequake monolith:** depends on `edgequake-extraction-core` as a normal Cargo dependency. No WASM involved on this side.
- **Worker:** the existing TypeScript shell stays in place. A new `kernel/` subdirectory in the Worker repo builds `edgequake-extraction-core` to WASM via `wasm-pack build --target web` with `#[wasm_bindgen]` exports for the public API. Wrangler bundles the resulting `.wasm` file plus its JS glue module into the Worker artifact alongside the TypeScript code.

**Critically, this remains a single Worker.** The WASM module is not a separate service. It loads into the same isolate as the TypeScript code on cold start, in a few milliseconds, and every request calls Rust functions through the `wasm-bindgen` glue with zero IPC, zero network hops, zero extra subrequests, and zero additional deployment. From Cloudflare's perspective and from the caller's perspective, it is indistinguishable from a pure TypeScript Worker. One Worker, one deploy artifact, one `wrangler deploy` command.

What stays in TypeScript:

- HTTP routing, request/response shaping
- Bearer-token auth, header parsing
- Provider HTTP calls (`fetch()` to OpenAI / Anthropic / Gemini) — these require the Workers `fetch` binding and are awkward from WASM
- Workers Secrets and env bindings
- Observability, metrics, structured logging
- KV / R2 / D1 access (for v1.1 idempotency, etc.)

What moves into the Rust kernel:

- `build_system_prompt(entity_types, language) -> String`
- `build_user_prompt(text, entity_types, language) -> String`
- `build_continue_prompt(language) -> String`
- `parse_response(raw, tuple_delim, completion_delim) -> ParsedTuples`
- `normalize_entity_name(input) -> String`
- `dedupe_and_merge(passes) -> (entities, relationships)`
- `list_presets() -> PresetMap`

All pure, synchronous, no I/O. They map cleanly onto `#[wasm_bindgen]` exports returning JSON-serializable structs that the TypeScript shell awaits like any async function.

Trade-offs vs a hypothetical pure-TS approach (for reference):

| Aspect | Pure TS (v1.0) | Hybrid kernel (v2.0) |
|---|---|---|
| Contract source of truth | shared text files (Layer 1) | shared compiled code |
| Drift risk | possible, caught by Layer 2 | impossible by construction |
| Bundle size | ~50–100 KB | + 200–400 KB WASM (after `wasm-opt -Oz`) |
| Cold start | ~5–10 ms | ~10–25 ms (one-time per isolate) |
| Per-request overhead | n/a | microseconds (JS↔WASM string crossing) |
| CI complexity | npm + Wrangler | + Rust toolchain + `wasm-pack` |
| Build time (cold) | ~30 s | ~2 min (~30 s with sccache/cargo cache) |
| Contributor surface | TypeScript only | TypeScript + Rust + WASM |
| Worker count, deploy count | 1, 1 | 1, 1 |
| Subrequest cost | unchanged | unchanged |

Cold-start cost is one-time per isolate and three to four orders of magnitude smaller than the dominant LLM call. Bundle size stays well within the 1 MB Workers limit. Per-request boundary crossing is microseconds for typical chunk sizes — the parser is called once per pass, not once per token.

The hybrid architecture ships from day one, eliminating the need for trigger criteria or a phased migration. The technical reference for the kernel build, repo layout, and `wasm-bindgen` interface is in **Appendix F**.

### 19.6 Layer 5 — Process and tooling (v1.0, required)

Three process commitments beyond code-level mitigations:

**Upstream watch automation.** A GitHub Action in the Worker repo subscribes to changes in `edgequake/contract/**` and `edgequake/crates/edgequake-pipeline/src/extractor.rs`. Any change opens an automatic PR in the Worker repo labeled `upstream-change` and assigns the ML lead. No upstream contract change ships to production without an explicit acknowledgment recorded in the Worker repo.

**Release coordination.** A `RELEASE-PARITY.md` in both repos records which Worker version is parity-tested against which edgequake version. Worker releases are cut against tagged edgequake releases, never `main`. Both repos publish the compatibility matrix in their READMEs.

**Production sampling.** The shadow-phase parallel-extraction mechanism from §15 Phase 1 is NOT torn down at full cutover. It continues to run on 1% of production traffic indefinitely, comparing Worker output against the inline Rust output (during the transition) or against a periodic offline replay through the monolith (post-transition). Discrepancies fire alarms; sustained discrepancy rates above 1% are treated as P2 incidents.

### 19.7 Maintenance acceptance criteria for v1.0

For v1.0 ship, the maintenance plan requires:

- [ ] Shared Rust kernel (`edgequake-extraction-core` crate) building to WASM and passing all tests.
- [ ] `contract/VERSION` file in place; version embedded in every Worker response and log line.
- [ ] Vendoring mechanism (submodule or Cargo git dependency) chosen, documented, and operational.
- [ ] Parity corpus contains ≥ 100 cases spanning all 6 presets.
- [ ] Both repos run the corpus in CI on every PR, failing on divergence.
- [ ] CI gate in edgequake repo prevents merging contract changes without a VERSION bump.
- [ ] Upstream-watch GitHub Action live and tested with a synthetic change.
- [ ] `RELEASE-PARITY.md` created in both repos and populated with the v1.0 compatibility row.
- [ ] Production-sampling mechanism documented and ready to run from cutover day.

---

## Appendix A — Prompt templates (verbatim from `entity_extraction.rs`)

**Placeholders:**
- `{entity_types}` — comma-joined list, e.g. `"PERSON, ORGANIZATION, LOCATION"`
- `{language}` — e.g. `"English"`
- `{tuple_delimiter}` — default `<|#|>`
- `{completion_delimiter}` — default `<|COMPLETE|>`
- `{examples}` — the few-shot example block (see Rust source for the three full examples; reproduce byte-identically)
- `{input_text}` — the chunk under extraction

### A.1 System prompt

Source: `edgequake/crates/edgequake-pipeline/src/prompts/entity_extraction.rs::system_prompt`.

Reproduce the format string verbatim. The full text begins with `---Role---\nYou are a Knowledge Graph Specialist...` and ends with `\n---Examples---\n{examples}`. Implementers MUST copy the string from the Rust source character-for-character (excluding the leading `r#"` and trailing `"#`) to ensure LLM contract parity.

### A.2 User prompt

Source: `entity_extraction.rs::user_prompt`. Begins `---Task---\nExtract entities and relationships from the input text below.` and ends `<Output>`. Same parity requirement.

### A.3 Continue (gleaning) prompt

Source: `entity_extraction.rs::continue_extraction_prompt`. Begins `---Task---\nBased on the last extraction task, identify and extract any **missed or incorrectly formatted** entities and relationships...` Same parity requirement.

### A.4 Few-shot examples

Three examples are interpolated into the system prompt's `{examples}` placeholder. They are NOT preset-aware (this is a known limitation, see OQ-8). Source: `entity_extraction.rs::get_examples`. Implementers MUST reproduce byte-identically.

---

## Appendix B — Preset definitions

To be finalized by ML lead. Initial proposal:

```jsonc
{
  "general": [
    "PERSON", "ORGANIZATION", "LOCATION", "EVENT",
    "CONCEPT", "TECHNOLOGY", "PRODUCT", "OTHER"
  ],
  "manufacturing": [
    "EQUIPMENT", "COMPONENT", "PROCESS", "MATERIAL",
    "DEFECT", "MEASUREMENT", "STANDARD", "FACILITY",
    "OPERATOR", "PRODUCT", "ORGANIZATION", "OTHER"
  ],
  "healthcare": [
    "PATIENT", "CONDITION", "MEDICATION", "PROCEDURE",
    "PROVIDER", "FACILITY", "SYMPTOM", "DIAGNOSIS",
    "ANATOMY", "DEVICE", "ORGANIZATION", "OTHER"
  ],
  "legal": [
    "PARTY", "STATUTE", "CASE", "COURT", "JURISDICTION",
    "OBLIGATION", "RIGHT", "PROVISION", "DATE", "MONETARY_AMOUNT",
    "ORGANIZATION", "PERSON", "OTHER"
  ],
  "research": [
    "AUTHOR", "PUBLICATION", "METHOD", "DATASET", "METRIC",
    "INSTITUTION", "FUNDER", "CONCEPT", "FINDING",
    "EXPERIMENT", "OTHER"
  ],
  "finance": [
    "INSTRUMENT", "ENTITY", "TRANSACTION", "MARKET",
    "METRIC", "REGULATION", "PERSON", "ORGANIZATION",
    "DATE", "MONETARY_AMOUNT", "EVENT", "OTHER"
  ]
}
```

Final lists subject to ML review against representative corpora.

---

## Appendix C — Tuple grammar

EBNF-ish notation:

```
response       := line ( "\n" line )* ;
line           := tuple_line | completion | ignored ;
completion     := completion_delimiter ;        // terminates parsing
ignored        := /.*/ ;                        // any line that doesn't parse as a tuple
tuple_line     := entity_tuple | relation_tuple ;
entity_tuple   := "entity" td name td type td description ;
relation_tuple := "relation" td source td target td keywords td description ;
td             := tuple_delimiter ;
name, source, target, type, keywords, description := /[^<\n]*/ ;
```

**Parsing algorithm (deterministic):**

```pseudo
function parse(response, tuple_delim, completion_delim) -> (entities, relations, stats):
    entities = []
    relations = []
    complete = false
    malformed_dropped = 0

    for line in response.split("\n"):
        line = line.strip()
        if line.is_empty(): continue
        if line == completion_delim:
            complete = true
            break

        fields = line.split(tuple_delim).map(strip)
        if fields[0] == "entity":
            if fields.len() != 4:
                malformed_dropped += 1
                continue
            entities.push({
                name: normalize(fields[1]),
                type: fields[2],
                description: fields[3],
            })
        elif fields[0] == "relation":
            if fields.len() != 5:
                malformed_dropped += 1
                continue
            relations.push({
                source: normalize(fields[1]),
                target: normalize(fields[2]),
                keywords: fields[3].split(",").map(strip).filter(non_empty),
                description: fields[4],
            })
        else:
            // commentary, code fences, blank padding — ignored
            malformed_dropped += 1

    return (entities, relations, { complete, malformed_dropped })
```

**Tolerance rules:**
- Surrounding whitespace on lines and fields: trimmed.
- Code fences (```), markdown headers, commentary: dropped.
- Truncated final line (no completion delimiter): partial output returned; `complete = false`.
- Empty fields: treated as empty strings, not parse errors. Empty entity names trigger dedupe collapse.

---

## Appendix D — Normalization algorithm

Source: `edgequake/crates/edgequake-pipeline/src/prompts/normalizer.rs` (reference implementation).

```pseudo
function normalize(name: string) -> string:
    s = name.trim()
    // Replace any non-alphanumeric, non-Unicode-letter character with a space
    s = s.replace(/[^\p{L}\p{N}]+/g, " ")
    s = s.trim()
    // Collapse runs of whitespace to single underscore
    s = s.replace(/\s+/g, "_")
    // Uppercase (locale-independent)
    s = s.to_uppercase()
    return s
```

**Examples (must match reference):**

| Input | Output |
|---|---|
| `"John Doe"` | `JOHN_DOE` |
| `"john doe"` | `JOHN_DOE` |
| `"the Company"` | `THE_COMPANY` |
| `"John's team"` | `JOHN_S_TEAM` |
| `"  Sarah  Chen  "` | `SARAH_CHEN` |
| `"东京"` | `东京` (Unicode letters preserved, uppercased where applicable) |
| `""` | `""` (empty stays empty; dedupe drops it) |

Verify against `normalizer.rs` test cases; promote them to TypeScript tests verbatim.

---

## Appendix E — Sample request/response

**Request**

```bash
curl -X POST https://extraction.example.workers.dev/v1/extract \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "text": "Dr. Sarah Chen, lead researcher at Quantum Dynamics Lab in Boston, published a groundbreaking paper on quantum entanglement in Nature Physics journal. The study was funded by the National Science Foundation.",
    "preset": "research",
    "language": "English",
    "glean_passes": 1,
    "provider": "openai"
  }'
```

**Response**

```json
{
  "request_id": "01928f47-7e5e-7c4b-9b0a-9b0a9b0a9b0a",
  "entities": [
    { "name": "SARAH_CHEN", "type": "AUTHOR", "description": "Dr. Sarah Chen is the lead researcher at Quantum Dynamics Lab who published a paper on quantum entanglement.", "source_pass": 0 },
    { "name": "QUANTUM_DYNAMICS_LAB", "type": "INSTITUTION", "description": "Quantum Dynamics Lab is a research institution located in Boston.", "source_pass": 0 },
    { "name": "BOSTON", "type": "OTHER", "description": "Boston is a city where Quantum Dynamics Lab is located.", "source_pass": 0 },
    { "name": "NATURE_PHYSICS", "type": "PUBLICATION", "description": "Nature Physics is a scientific journal that published Sarah Chen's paper.", "source_pass": 0 },
    { "name": "QUANTUM_ENTANGLEMENT", "type": "CONCEPT", "description": "Quantum entanglement is a physics phenomenon studied in Sarah Chen's groundbreaking paper.", "source_pass": 0 },
    { "name": "NATIONAL_SCIENCE_FOUNDATION", "type": "FUNDER", "description": "The National Science Foundation funded Sarah Chen's quantum entanglement research.", "source_pass": 0 }
  ],
  "relationships": [
    { "source": "SARAH_CHEN", "target": "QUANTUM_DYNAMICS_LAB", "keywords": ["employment", "research"], "description": "Sarah Chen works as lead researcher at Quantum Dynamics Lab.", "source_pass": 0 },
    { "source": "SARAH_CHEN", "target": "NATURE_PHYSICS", "keywords": ["publication"], "description": "Sarah Chen published her research in Nature Physics journal.", "source_pass": 0 },
    { "source": "SARAH_CHEN", "target": "QUANTUM_ENTANGLEMENT", "keywords": ["research"], "description": "Sarah Chen researches quantum entanglement.", "source_pass": 0 },
    { "source": "QUANTUM_DYNAMICS_LAB", "target": "BOSTON", "keywords": ["location"], "description": "Quantum Dynamics Lab is located in Boston.", "source_pass": 0 },
    { "source": "NATIONAL_SCIENCE_FOUNDATION", "target": "SARAH_CHEN", "keywords": ["funding"], "description": "The National Science Foundation funded Sarah Chen's research.", "source_pass": 0 }
  ],
  "stats": {
    "input_tokens": 187,
    "output_tokens": 412,
    "passes_executed": 2,
    "complete_signal_received": true,
    "malformed_lines_dropped": 0,
    "duration_ms": 3140,
    "provider": "openai",
    "model": "gpt-4o-mini",
    "entity_types_resolved": ["AUTHOR", "PUBLICATION", "METHOD", "DATASET", "METRIC", "INSTITUTION", "FUNDER", "CONCEPT", "FINDING", "EXPERIMENT", "OTHER"]
  },
  "warnings": []
}
```

---

## Appendix F — Hybrid kernel architecture (v1.0 technical reference)

This appendix is the technical reference for the hybrid architecture: a TypeScript Worker shell wrapping a shared Rust kernel compiled to WASM. **A single Worker, single deployment, single isolate per request.** The kernel is a bundled asset, not a separate service. This ships from day one.

### F.1 Repository layout

```
extraction-worker/
├── wrangler.toml
├── package.json
├── tsconfig.json
├── src/                          ← TypeScript (Worker shell, unchanged from v1.0)
│   ├── index.ts                  router, fetch handler
│   ├── auth.ts                   Bearer-token validation
│   ├── providers/                HTTP calls to OpenAI / Anthropic / Gemini
│   │   ├── openai.ts
│   │   ├── anthropic.ts
│   │   └── gemini.ts
│   ├── obs.ts                    logging, metrics
│   ├── errors.ts                 error catalog and serializer
│   └── kernel.ts                 thin wrapper importing the WASM module
├── kernel/                       ← Rust (compiled to WASM, source-shared with monolith)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                #[wasm_bindgen] exports
│       ├── prompts.rs            template rendering
│       ├── parser.rs             tuple parsing
│       ├── normalizer.rs         name canonicalization
│       ├── dedupe.rs             merge logic
│       └── presets.rs            preset definitions
└── pkg/                          ← wasm-pack output (gitignored, built in CI)
    ├── kernel_bg.wasm
    └── kernel.js                 wasm-bindgen JS glue
```

The `kernel/` directory either vendors the `edgequake-extraction-core` crate from the edgequake repo (git submodule or Cargo path dependency) or pulls it as a published crate. The monolith depends on the same crate directly via Cargo.

### F.2 Build pipeline

```bash
# In the kernel/ directory:
wasm-pack build --target web --release

# Optionally optimize:
wasm-opt -Oz pkg/kernel_bg.wasm -o pkg/kernel_bg.wasm

# Wrangler bundles src/ + pkg/ into one Worker artifact:
wrangler deploy
```

`wrangler.toml` declares the WASM module as a binding or imports it directly via the TS code (Wrangler's default bundler handles `import wasmModule from "../pkg/kernel_bg.wasm"`).

CI adds two steps before `wrangler deploy`:

1. `rustup target add wasm32-unknown-unknown` (cached)
2. `cd kernel && wasm-pack build --target web --release`

With `sccache` or Cargo registry caching, this adds ~30 s to a warm CI run.

### F.3 `wasm-bindgen` interface (sketch)

```rust
// kernel/src/lib.rs
use wasm_bindgen::prelude::*;
use serde::Serialize;

#[wasm_bindgen]
pub fn build_system_prompt(entity_types_json: &str, language: &str) -> String {
    let types: Vec<String> = serde_json::from_str(entity_types_json).unwrap();
    edgequake_extraction_core::prompts::system_prompt(&types, language)
}

#[wasm_bindgen]
pub fn parse_response(
    raw: &str,
    tuple_delim: &str,
    completion_delim: &str,
) -> Result<JsValue, JsValue> {
    let parsed = edgequake_extraction_core::parser::parse(raw, tuple_delim, completion_delim);
    serde_wasm_bindgen::to_value(&parsed).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn normalize_entity_name(name: &str) -> String {
    edgequake_extraction_core::normalizer::normalize(name)
}

// ... build_user_prompt, build_continue_prompt, dedupe_and_merge, list_presets
```

```typescript
// src/kernel.ts
import init, {
  build_system_prompt,
  build_user_prompt,
  build_continue_prompt,
  parse_response,
  normalize_entity_name,
  dedupe_and_merge,
} from "../pkg/kernel.js";

let ready: Promise<void> | null = null;

export async function initKernel(): Promise<void> {
  if (!ready) ready = init();
  await ready;
}

export function buildSystemPrompt(types: string[], language: string): string {
  return build_system_prompt(JSON.stringify(types), language);
}

export function parseResponse(raw: string, td: string, cd: string): ParsedTuples {
  return parse_response(raw, td, cd) as ParsedTuples;
}

// ... etc
```

The TS shell calls `initKernel()` once per isolate (lazy, on first request) and then invokes the Rust functions synchronously inside the request handler.

### F.4 Boundary performance

Strings cross the JS↔WASM boundary as UTF-16↔UTF-8 conversions. For typical chunk sizes (a few KB of input text, a few KB of LLM output) the conversion is microseconds. Guidance:

- Call `parse_response` **once** per pass, not per line. The kernel returns a structured object; the shell does not re-parse.
- Pass arrays (e.g. `entity_types`) as JSON strings rather than individual JS↔WASM array marshaling. Simpler and roughly equivalent in cost for small sizes.
- Avoid passing the same large string across the boundary multiple times.

### F.5 Cold-start cost

WASM instantiation on first request to a fresh isolate is ~5–20 ms. Workers keep isolates warm under load, so this is amortized to near-zero per request. For comparison: a single LLM call is 1,000–10,000 ms. The cold-start contribution to P99 latency is negligible.

### F.6 Bundle size budget

The 1 MB Workers bundle ceiling covers the combined TS + WASM artifact. For the extraction kernel:

- TS shell + provider adapters + dependencies (no large libraries): ~50–100 KB.
- Rust kernel WASM after `wasm-opt -Oz`: ~200–400 KB.
- Total: well under 1 MB with ample headroom.

If size becomes a concern, the kernel can be further trimmed with `wee_alloc`, panic-free builds, and aggressive dead-code elimination — but for the scope here, default `--release` builds are sufficient.

### F.7 Toolchain alternative: `workers-rs`

For teams that prefer a pure-Rust Worker (no TypeScript shell at all), `workers-rs` is the alternative toolchain. The kernel crate is consumed directly; HTTP routing, auth, and provider calls are written in Rust using the `worker` crate's `Fetch`, `Request`, `Response` types. Trade-offs vs the hybrid model: simpler architecture for Rust-only teams, but loses the TypeScript ecosystem for the shell-level code (zod for validation, hono or itty-router for routing, etc.) and increases the surface area that must be Rust-fluent for any contributor. The PRD does not recommend this path for the typical team but documents it as a valid option if the team prefers it.

### F.8 Kernel crate sourcing

The `kernel/` directory vendors the extraction logic from the edgequake repository. Two sourcing strategies are supported:

1. **Git submodule** — `kernel/` points to a subdirectory of the edgequake repo. CI checks out the pinned commit.
2. **Cargo path/git dependency** — `kernel/Cargo.toml` declares a git dependency on the `edgequake-extraction-core` crate with a pinned tag.

Either approach ensures the kernel is always at a known, tested version. CI in both repos enforces that the parity corpus passes for any pinned combination.

---

## Appendix G — References

- edgequake repository: https://github.com/raphaelmansuy/edgequake
- LightRAG paper: https://arxiv.org/abs/2410.05779
- Cloudflare Workers docs: https://developers.cloudflare.com/workers/
- Google Gemini API: https://ai.google.dev/docs
- `workers-rs`: https://github.com/cloudflare/workers-rs
- `wasm-pack`: https://github.com/rustwasm/wasm-pack
- `wasm-bindgen`: https://github.com/rustwasm/wasm-bindgen
- `serde-wasm-bindgen`: https://github.com/RReverser/serde-wasm-bindgen
- `wasm-opt` (Binaryen): https://github.com/WebAssembly/binaryen
- `minijinja` (template rendering for shared contract files): https://github.com/mitsuhiko/minijinja
- Internal docs:
  - `docs/concepts/entity-extraction.md`
  - `docs/deep-dives/lightrag-algorithm.md`
  - `edgequake/crates/edgequake-pipeline/src/extractor.rs`
  - `edgequake/crates/edgequake-pipeline/src/prompts/entity_extraction.rs`
  - `edgequake/crates/edgequake-pipeline/src/prompts/parser.rs`
  - `edgequake/crates/edgequake-pipeline/src/prompts/normalizer.rs`

---

*End of PRD.*
