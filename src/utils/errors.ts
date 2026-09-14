/** Human-readable text for a rejected invoke: cloud `ApiError`s arrive as `{status, code, message}` objects. */
export function describeError(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object') {
    const o = e as { code?: unknown; status?: unknown; message?: unknown };
    if (typeof o.message === 'string' && o.message) return o.message;
    if (typeof o.code === 'string') {
      return typeof o.status === 'number' && o.status > 0 ? `${o.code} (HTTP ${o.status})` : o.code;
    }
    try {
      return JSON.stringify(e);
    } catch {
      /* circular — fall through */
    }
  }
  return String(e);
}
