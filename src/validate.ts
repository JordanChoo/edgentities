import { z } from "zod";
import { AppError } from "./errors.js";

const ENTITY_TYPE_PATTERN = /^[A-Z][A-Z0-9_]{0,63}$/;
const PRESET_NAMES = ["general", "manufacturing", "healthcare", "legal", "research", "finance"] as const;
const PROVIDER_NAMES = ["openai", "anthropic", "gemini"] as const;

const extractRequestSchema = z
  .object({
    text: z.string().min(1),
    entity_types: z
      .array(z.string().regex(ENTITY_TYPE_PATTERN))
      .min(1)
      .max(50)
      .optional(),
    preset: z.enum(PRESET_NAMES).optional(),
    language: z.string().default("English"),
    glean_passes: z.number().int().min(0).max(3).default(1),
    provider: z.enum(PROVIDER_NAMES).optional(),
    model: z.string().optional(),
    tuple_delimiter: z
      .string()
      .min(1)
      .refine((s) => !s.includes("\n"), "Must not contain newlines")
      .default("<|#|>"),
    completion_delimiter: z
      .string()
      .min(1)
      .refine((s) => !s.includes("\n"), "Must not contain newlines")
      .default("<|COMPLETE|>"),
    merge_descriptions: z.boolean().default(false),
    temperature: z.number().min(0).max(2).default(0.0),
    max_output_tokens: z.number().int().positive().optional(),
  })
  .refine(
    (data) => !(data.entity_types && data.preset),
    "entity_types and preset are mutually exclusive",
  );

export type ExtractRequest = z.infer<typeof extractRequestSchema>;

export function validateExtractRequest(body: unknown, maxChars: number): ExtractRequest {
  const result = extractRequestSchema.safeParse(body);
  if (!result.success) {
    const message = result.error.issues.map((i) => `${i.path.join(".")}: ${i.message}`).join("; ");
    throw new AppError("INVALID_REQUEST", message);
  }

  if (result.data.text.length > maxChars) {
    throw new AppError("CONTENT_TOO_LARGE", `Text exceeds maximum of ${maxChars} characters`);
  }

  return result.data;
}
