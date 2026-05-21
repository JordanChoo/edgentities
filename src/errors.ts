export type ErrorCode =
  | "INVALID_REQUEST"
  | "UNAUTHORIZED"
  | "FORBIDDEN"
  | "CONTENT_TOO_LARGE"
  | "RATE_LIMITED"
  | "PROVIDER_RATE_LIMITED"
  | "PROVIDER_AUTH_ERROR"
  | "PROVIDER_ERROR"
  | "PROVIDER_TIMEOUT"
  | "EXTRACTION_EMPTY"
  | "INTERNAL_ERROR";

const ERROR_HTTP_STATUS: Record<ErrorCode, number> = {
  INVALID_REQUEST: 400,
  UNAUTHORIZED: 401,
  FORBIDDEN: 403,
  CONTENT_TOO_LARGE: 413,
  RATE_LIMITED: 429,
  PROVIDER_RATE_LIMITED: 429,
  PROVIDER_AUTH_ERROR: 502,
  PROVIDER_ERROR: 502,
  PROVIDER_TIMEOUT: 504,
  EXTRACTION_EMPTY: 422,
  INTERNAL_ERROR: 500,
};

const RETRYABLE: Set<ErrorCode> = new Set([
  "RATE_LIMITED",
  "PROVIDER_RATE_LIMITED",
  "PROVIDER_ERROR",
  "PROVIDER_TIMEOUT",
  "INTERNAL_ERROR",
]);

export class AppError extends Error {
  code: ErrorCode;
  httpStatus: number;
  retryable: boolean;
  details?: Record<string, unknown>;

  constructor(code: ErrorCode, message: string, details?: Record<string, unknown>) {
    super(message);
    this.code = code;
    this.httpStatus = ERROR_HTTP_STATUS[code];
    this.retryable = RETRYABLE.has(code);
    this.details = details;
  }
}

export function serializeError(err: AppError, requestId: string) {
  return {
    error: {
      code: err.code,
      message: err.message,
      retryable: err.retryable,
      ...(err.details ? { details: err.details } : {}),
    },
    request_id: requestId,
  };
}
