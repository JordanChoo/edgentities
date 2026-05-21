# edgentities

A stateless entity and relationship extraction service deployed on Cloudflare Workers. Given a block of text, edgentities uses LLMs to identify entities (people, organizations, locations, concepts, etc.) and the relationships between them, returning structured JSON suitable for knowledge graph construction.

The service is a hybrid TypeScript/Rust architecture: a TypeScript shell handles HTTP routing, authentication, request validation, and LLM provider orchestration, while a Rust kernel compiled to WebAssembly handles the deterministic, CPU-bound work of prompt construction, response parsing, entity name normalization, and cross-pass deduplication.

## Why edgentities exists

Entity extraction is the first step in building knowledge graphs from unstructured text. While the LLM does the heavy cognitive work of identifying entities and their connections, the surrounding infrastructure (prompt engineering, output parsing, name normalization, multi-pass gleaning, deduplication) is substantial and benefits from being isolated as a standalone service:

- **Stateless by design.** No database, no file system, no long-lived connections. One text chunk in, one JSON object out.
- **Geographically distributed.** Cloudflare Workers run in 300+ edge locations. The only wide-area network hop is the LLM API call itself.
- **Provider-agnostic.** Route requests to OpenAI, Anthropic, or Google Gemini on a per-request basis depending on cost, latency, or quality requirements.
- **Language-independent.** Extract entities in any language the LLM supports; all prompts and outputs respect the configured language parameter.
- **Decoupled.** Any HTTP client can call the extraction endpoint. No SDK, no language binding, no monolith dependency.

## Architecture

```
                    +-----------------------+
                    |   Cloudflare Worker   |
                    |                       |
  HTTP Request ---->  TypeScript Shell     |
                    |   - Hono router       |
                    |   - Auth (?csvkey=)   |
                    |   - Zod validation    |
                    |   - Provider dispatch |
                    |   - Gleaning loop     |
                    |         |             |
                    |         v             |
                    |   Rust/WASM Kernel    |
                    |   - Prompt building   |
                    |   - Tuple parsing     |
                    |   - Name normalizer   |
                    |   - Cross-pass dedup  |
                    |   - Preset registry   |
                    +-----------+-----------+
                                |
                                v
                    OpenAI / Anthropic / Gemini
```

The TypeScript shell and Rust kernel run in the same V8 isolate. The WASM module is bundled at deploy time and initialized lazily on first request. There is no cold-start penalty beyond the sub-millisecond WASM instantiation.

## Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/v1/health` | No | Load balancer probe. Always returns `200`. |
| `GET` | `/v1/version` | No | Returns service version and build metadata. |
| `GET` | `/v1/ready` | Yes | Returns `200` if at least one LLM provider key is configured, `503` otherwise. |
| `GET` | `/v1/presets` | Yes | Lists all available entity-type presets. |
| `POST` | `/v1/extract` | Yes | Performs entity and relationship extraction. |

## Quick start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)
- [Node.js](https://nodejs.org/) (v18+)
- A Cloudflare account (for deployment) or just `wrangler` for local dev

### Local development

```bash
# Install dependencies
npm install

# Build the WASM kernel
npm run build:kernel

# Set up local secrets
echo 'API_KEYS=your-auth-key' > .dev.vars
echo 'GEMINI_API_KEY=your-gemini-key' >> .dev.vars

# Start the dev server
npm run dev
```

### Making a request

```bash
curl -X POST "http://localhost:8787/v1/extract?csvkey=your-auth-key" \
  -H "Content-Type: application/json" \
  -d '{
    "text": "Apple was founded by Steve Jobs and Steve Wozniak in 1976 in Los Altos, California.",
    "provider": "gemini",
    "preset": "general"
  }'
```

## Deployment

### First-time setup

1. **Log in to Cloudflare.** This opens a browser to authorize Wrangler against your Cloudflare account:

   ```bash
   wrangler login
   ```

2. **Verify your account.** Confirm Wrangler can see your account and has the right permissions:

   ```bash
   wrangler whoami
   ```

3. **Configure secrets.** Each command prompts you to paste the value interactively. Secrets must be set _before_ the first deploy; otherwise the Worker will return errors on every request.

   `API_KEYS` is the authentication secret — the value you set here is what callers must pass as the `?csvkey=` query parameter. You can set multiple keys (comma-separated) for different consumers:

   ```bash
   wrangler secret put API_KEYS
   # When prompted, enter your key(s): e.g. "my-secret-key" or "key1,key2,key3"
   ```

   Then add at least one LLM provider key:

   ```bash
   wrangler secret put GEMINI_API_KEY
   ```

   Optionally, if you plan to use OpenAI or Anthropic:

   ```bash
   wrangler secret put OPENAI_API_KEY
   wrangler secret put ANTHROPIC_API_KEY
   ```

   You can verify which secrets are configured (values are not shown):

   ```bash
   wrangler secret list
   ```

4. **Deploy.** This compiles the Rust kernel to WebAssembly via `wasm-pack`, bundles it with the TypeScript shell, and pushes to Cloudflare's edge network:

   ```bash
   wrangler deploy
   ```

   Wrangler prints the live URL on success (e.g., `https://extraction-worker.<your-subdomain>.workers.dev`).

5. **Verify the deployment.** Run a quick health check against the live URL:

   ```bash
   # Should return {"status":"ok"} (health endpoint is unauthenticated)
   curl -s https://extraction-worker.<your-subdomain>.workers.dev/v1/health | jq .

   # Full end-to-end test
   curl -s -X POST "https://extraction-worker.<your-subdomain>.workers.dev/v1/extract?csvkey=YOUR_KEY" \
     -H "Content-Type: application/json" \
     -d '{"text": "Test extraction.", "provider": "gemini"}' | jq .
   ```

### Deploying via the Cloudflare dashboard

If you prefer not to use the Wrangler CLI, you can deploy directly through the Cloudflare dashboard by connecting your GitHub repository.

1. Log in to the [Cloudflare dashboard](https://dash.cloudflare.com/) and navigate to **Workers & Pages**.
2. Click **Create** and select **Import a repository**.
3. Connect your GitHub account if you haven't already, then select the `edgentities` repository.
4. Configure the build settings:
   - **Build command:** `npx wrangler deploy`
   - **Root directory:** leave empty (the project root contains `wrangler.toml`)
5. Click **Deploy**. Cloudflare will clone the repo, install dependencies, compile the Rust kernel to WebAssembly, and deploy the Worker. The build environment must have Rust and `wasm-pack` installed; Cloudflare's build system includes Rust by default, but you may need to add `wasm-pack` installation to the build command: `curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh && npx wrangler deploy`
6. After the first deploy completes, go to **Settings > Variables and Secrets** for the Worker and add the required secrets:
   - `API_KEYS` — the authentication key(s) that callers must provide in the `?csvkey=` query parameter. If you need multiple keys (e.g., one per consumer), enter them as a comma-separated list (e.g., `key-for-app-a,key-for-app-b`). Every request to a protected endpoint must include `?csvkey=<one-of-these-values>` or it will be rejected with a `401`.
   - `GEMINI_API_KEY` — your Google Generative AI API key (required if using the `gemini` provider)
   - `OPENAI_API_KEY` — your OpenAI API key (required if using the `openai` provider)
   - `ANTHROPIC_API_KEY` — your Anthropic API key (required if using the `anthropic` provider)

   Click **Encrypt** for each value to store them as encrypted secrets. You only need to add provider keys for the providers you plan to use — at least one is required for the Worker to be functional.

With this setup, every push to `main` triggers an automatic rebuild and deploy. You can disable automatic deployments or limit them to specific branches under **Settings > Builds & Deployments**.

### Updating after code changes

Redeployment is a single command. Wrangler rebuilds and pushes atomically:

```bash
wrangler deploy
```

The new version goes live globally within seconds. There is no need to re-configure secrets; they persist across deployments.

### Environment management

The `wrangler.toml` already defines staging and production environments. Deploy and configure secrets per environment:

```bash
# Staging
wrangler deploy --env staging
wrangler secret put API_KEYS --env staging
wrangler secret put GEMINI_API_KEY --env staging

# Production
wrangler deploy --env production
wrangler secret put API_KEYS --env production
wrangler secret put GEMINI_API_KEY --env production
```

This lets you test with a separate `API_KEYS` value and provider keys in staging before promoting to production. Each environment gets its own URL and its own set of secrets.

### Rollback

Cloudflare Workers supports instant rollback to any previous deployment version through the dashboard or CLI.

**Via CLI:** list recent deployments and roll back by version ID:

```bash
wrangler deployments list
wrangler deployments rollback <version-id>
```

**Via dashboard:** navigate to **Workers & Pages > extraction-worker > Deployments**, find the version you want, and click **Rollback**.

Rollbacks are instant and do not affect secrets; the previous Worker code runs with the current secret values. If you need to roll back a secret change, use `wrangler secret put` to set the previous value.

### Monitoring a deployment

After deploying, you can tail live logs to watch requests in real time:

```bash
wrangler tail
```

Use `wrangler tail --format json` for structured output suitable for piping into log aggregation tools.

## Request schema

The `POST /v1/extract` endpoint accepts a JSON body with the following fields:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `text` | string | *required* | The text to extract entities from. Minimum 1 character; upper bound set by `MAX_INPUT_CHARS` (default 32,000). |
| `provider` | `"openai"` \| `"anthropic"` \| `"gemini"` | `"openai"` | Which LLM provider to use. Falls back to `DEFAULT_PROVIDER` env var, then `openai`. |
| `entity_types` | string[] | — | Custom entity type list. Mutually exclusive with `preset`. |
| `preset` | string | `"general"` | Named preset for entity types. Mutually exclusive with `entity_types`. |
| `language` | string | `"English"` | Language for extraction output. |
| `glean_passes` | 0-3 | `1` | Number of additional "what did you miss?" passes. |
| `model` | string | — | Override the provider's default model. |
| `temperature` | 0-2 | `0` | LLM sampling temperature. |
| `max_output_tokens` | integer | — | Cap on LLM output length. |
| `tuple_delimiter` | string | `<\|#\|>` | Field separator in LLM output. |
| `completion_delimiter` | string | `<\|COMPLETE\|>` | End-of-output sentinel. |
| `merge_descriptions` | boolean | `false` | Concatenate duplicate descriptions instead of keeping longest. |

## Response schema

```json
{
  "request_id": "uuid",
  "entities": [
    {
      "name": "STEVE_JOBS",
      "type": "PERSON",
      "description": "Steve Jobs was a co-founder of Apple Inc.",
      "source_pass": 0
    }
  ],
  "relationships": [
    {
      "source": "APPLE_INC.",
      "target": "STEVE_JOBS",
      "keywords": ["founding", "leadership"],
      "description": "Steve Jobs co-founded Apple Inc.",
      "source_pass": 0
    }
  ],
  "stats": {
    "input_tokens": 2048,
    "output_tokens": 512,
    "passes_executed": 2,
    "complete_signal_received": true,
    "malformed_lines_dropped": 0,
    "duration_ms": 8500,
    "provider": "gemini",
    "model": "gemini-2.5-flash",
    "entity_types_resolved": ["PERSON", "ORGANIZATION", "LOCATION", "..."]
  },
  "warnings": []
}
```

## Presets

Presets define curated entity-type lists for different domains:

| Preset | Entity types |
|--------|-------------|
| **general** | PERSON, ORGANIZATION, LOCATION, EVENT, CONCEPT, TECHNOLOGY, PRODUCT, OTHER |
| **manufacturing** | EQUIPMENT, COMPONENT, PROCESS, MATERIAL, DEFECT, MEASUREMENT, STANDARD, FACILITY, OPERATOR, PRODUCT, ORGANIZATION, OTHER |
| **healthcare** | PATIENT, CONDITION, MEDICATION, PROCEDURE, PROVIDER, FACILITY, SYMPTOM, DIAGNOSIS, ANATOMY, DEVICE, ORGANIZATION, OTHER |
| **legal** | PARTY, STATUTE, CASE, COURT, JURISDICTION, OBLIGATION, RIGHT, PROVISION, DATE, MONETARY_AMOUNT, ORGANIZATION, PERSON, OTHER |
| **research** | AUTHOR, PUBLICATION, METHOD, DATASET, METRIC, INSTITUTION, FUNDER, CONCEPT, FINDING, EXPERIMENT, OTHER |
| **finance** | INSTRUMENT, ENTITY, TRANSACTION, MARKET, METRIC, REGULATION, PERSON, ORGANIZATION, DATE, MONETARY_AMOUNT, EVENT, OTHER |

Custom entity types can be passed directly via the `entity_types` field using any `UPPERCASE_UNDERSCORED` string (max 50 types per request).

## Authentication

All protected endpoints require the `csvkey` query parameter:

```
GET /v1/presets?csvkey=<key>
POST /v1/extract?csvkey=<key>
```

API keys are configured as a comma-separated environment variable (`API_KEYS`), allowing rotation without redeployment. Authentication uses constant-time comparison to prevent timing attacks.

## How extraction works

### The gleaning loop

Extraction runs in multiple passes to maximize recall:

1. **Pass 0 (initial extraction):** The system prompt instructs the LLM to act as a Knowledge Graph Specialist. The user prompt injects the text and requested entity types. The LLM outputs structured tuples.

2. **Passes 1-N (gleaning):** The LLM's previous response is appended to the conversation history, followed by a "continue" prompt asking it to identify anything it missed or formatted incorrectly. This iterative refinement catches entities that were overlooked in the initial pass.

3. **Early termination:** If a gleaning pass produces zero new entities and zero new relationships, further passes are skipped.

The number of gleaning passes (0-3) is configurable per request. More passes increase recall at the cost of additional LLM calls and latency.

### Tuple-delimited output format

Rather than asking the LLM to produce JSON (which is brittle with structured extraction), edgentities uses a line-oriented tuple format:

```
entity<|#|>Steve Jobs<|#|>PERSON<|#|>Co-founder of Apple Inc.
entity<|#|>Apple<|#|>ORGANIZATION<|#|>Technology company founded in 1976.
relation<|#|>Steve Jobs<|#|>Apple<|#|>founding, leadership<|#|>Steve Jobs co-founded Apple.
<|COMPLETE|>
```

Each line is a record. Fields are separated by a configurable delimiter (`<|#|>` by default). The first field identifies the record type (`entity` or `relation`/`relationship`). A sentinel line (`<|COMPLETE|>`) signals that the LLM has finished outputting.

This format was chosen because:
- LLMs produce it more reliably than nested JSON for long outputs
- Partial output is still parseable (each line is independent)
- The sentinel enables detection of truncated responses
- Delimiters are unlikely to appear in natural text

### Entity name normalization

Raw entity names from LLM output are canonicalized through a deterministic pipeline:

1. **Trim** leading/trailing whitespace
2. **Strip English articles** (The, A, An) from the beginning
3. **Split** on whitespace (collapses multiple spaces, tabs, newlines)
4. **Remove possessives** ('s and Unicode curly apostrophe variants)
5. **Title-case** each word (first char upper, rest lower)
6. **Join** with underscore
7. **Uppercase** the entire result

This ensures that "the United States's", "United States", "UNITED STATES", and "united states" all resolve to `UNITED_STATES`, enabling reliable deduplication and graph merging downstream.

### Cross-pass deduplication

After all passes complete, entities and relationships are deduplicated:

**Entities** are grouped by normalized name. Within each group:
- The first non-`OTHER` type wins (gleaning passes often refine generic types)
- The longest description is kept, or all unique descriptions are concatenated with ` | ` if `merge_descriptions` is enabled
- The earliest `source_pass` is preserved for provenance

**Relationships** are grouped by an unordered `{source, target}` key (treating A→B and B→A as the same edge). Within each group:
- Keywords are unioned across all instances
- The same description merge policy applies as for entities
- Deterministic output ordering: sorted by source_pass, then source name, then target name

### Self-reference filtering

Relationships where the normalized source and target resolve to the same entity are silently dropped. This catches cases where the LLM outputs "The Company relates to Company" or "Apple Inc. is associated with Apple" which would create meaningless self-loops in a knowledge graph.

### Keyword capping

Each relationship edge is limited to 5 keywords maximum. This prevents the LLM from producing unbounded keyword lists that would bloat the graph schema.

## Design principles

### Hybrid architecture rationale

The TypeScript/Rust split is intentional and aligns responsibilities with each language's strengths:

**TypeScript handles I/O and orchestration:**
- HTTP routing (Hono provides ergonomic middleware, routing, and response helpers)
- Request validation (Zod provides schema inference and detailed error messages)
- LLM provider API calls (each provider has a different HTTP contract)
- Conversation history management across gleaning passes
- Abort signals and timeout enforcement

**Rust/WASM handles deterministic computation:**
- Prompt template construction (string-heavy, benefits from zero-copy slicing)
- Response parsing (line-by-line state machine, no allocations for skipped lines)
- Name normalization (Unicode-aware character manipulation)
- Deduplication (HashMap-based grouping with sorted output)
- Preset registry (static data, no runtime overhead)

This split means the TypeScript layer never parses LLM output or manipulates entity names, and the Rust layer never makes network calls or manages async state. Each side has a minimal, well-typed interface.

### Retry and backoff strategy

Provider calls use a progressive backoff schedule of 250ms, 1s, and 4s between retry attempts. Only transient failures are retried:

- **Retryable:** 408 (timeout), 429 (rate limit), 500, 502, 503, 504, network errors (DNS/TCP failures)
- **Not retryable:** 400 (bad request), 401/403 (auth), 404 (not found)

The maximum retry count is configurable via the `LLM_MAX_RETRIES` environment variable (default: 3).

### Error taxonomy

All errors are classified with a machine-readable code and appropriate HTTP status:

| Code | HTTP | Meaning |
|------|------|---------|
| `INVALID_REQUEST` | 400 | Request body failed validation |
| `UNAUTHORIZED` | 401 | Missing or invalid credentials |
| `FORBIDDEN` | 403 | Access denied |
| `CONTENT_TOO_LARGE` | 413 | Text exceeds MAX_INPUT_CHARS |
| `RATE_LIMITED` | 429 | Request rate limited |
| `PROVIDER_RATE_LIMITED` | 429 | Provider rate limited the request |
| `PROVIDER_AUTH_ERROR` | 502 | Provider rejected our credentials |
| `PROVIDER_ERROR` | 502 | Provider returned an unexpected error |
| `PROVIDER_TIMEOUT` | 504 | Provider did not respond in time |
| `EXTRACTION_EMPTY` | 422 | LLM returned no usable content |
| `INTERNAL_ERROR` | 500 | Unexpected server error |

Every error response includes the `request_id` for tracing.

## Project structure

```
edgentities/
+-- kernel/                  # Rust/WASM extraction kernel
|   +-- Cargo.toml
|   +-- src/
|       +-- lib.rs           # wasm-bindgen entry point
|       +-- normalizer.rs    # Entity name canonicalization
|       +-- parser.rs        # Tuple-delimited response parser
|       +-- prompts.rs       # LLM prompt templates
|       +-- presets.rs       # Domain entity-type presets
|       +-- dedupe.rs        # Cross-pass deduplication
+-- src/                     # TypeScript Worker shell
|   +-- index.ts             # Hono HTTP router
|   +-- auth.ts              # csvkey query parameter authentication
|   +-- validate.ts          # Zod request validation
|   +-- extractor.ts         # Gleaning loop orchestration
|   +-- kernel.ts            # WASM bridge (init + wrappers)
|   +-- errors.ts            # Error types and HTTP mapping
|   +-- types.ts             # Shared TypeScript interfaces
|   +-- wasm.d.ts            # Module declaration for .wasm imports
|   +-- providers/
|       +-- index.ts         # Provider factory
|       +-- openai.ts        # OpenAI Chat Completions
|       +-- anthropic.ts     # Anthropic Messages API
|       +-- gemini.ts        # Google Generative AI
|       +-- retry.ts         # Exponential backoff wrapper
+-- contract/                # Vendored edgequake reference code
+-- prd/                     # Product requirements document
+-- pkg/                     # Compiled WASM output (git-ignored)
+-- wrangler.toml            # Cloudflare Workers configuration
+-- package.json             # Node.js project manifest
+-- tsconfig.json            # TypeScript compiler configuration
```

## Environment variables

### Plaintext vars (wrangler.toml)

| Variable | Default | Description |
|----------|---------|-------------|
| `DEFAULT_PROVIDER` | `openai` | Provider used when request omits `provider` field |
| `OPENAI_DEFAULT_MODEL` | `gpt-4o-mini` | Default model for OpenAI requests |
| `ANTHROPIC_DEFAULT_MODEL` | `claude-sonnet-4-6` | Default model for Anthropic requests |
| `GEMINI_DEFAULT_MODEL` | `gemini-2.5-flash` | Default model for Gemini requests |
| `MAX_INPUT_CHARS` | `32000` | Maximum input text length |
| `LLM_TIMEOUT_MS` | `60000` | Per-call timeout for LLM requests |
| `LLM_MAX_RETRIES` | `3` | Maximum retry attempts per LLM call |

### Secrets (wrangler secret)

| Secret | Description |
|--------|-------------|
| `API_KEYS` | Comma-separated list of valid authentication keys |
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `GEMINI_API_KEY` | Google Generative AI API key |

## Scripts

| Script | Description |
|--------|-------------|
| `npm run build:kernel` | Compile Rust kernel to WASM via wasm-pack |
| `npm run build` | Build the WASM kernel (alias for build:kernel) |
| `npm run dev` | Start local Wrangler dev server |
| `npm run deploy` | Deploy to production |
| `npm run deploy:staging` | Deploy to staging |
| `npm run typecheck` | Run TypeScript type checking |
| `npm run test` | Run test suite |
| `npm run test:watch` | Run tests in watch mode |

## Acknowledgments

edgentities is derived from [edgequake](https://github.com/raphaelmansuy/edgequake) by Raphael Mansuy. The extraction kernel's prompt templates, tuple parser, entity name normalizer, and gleaning loop design are ported from edgequake's `edgequake-pipeline` crate. The original Rust source files are vendored in the `contract/` directory as a reference implementation.

## License

MIT
