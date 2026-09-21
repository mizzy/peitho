export async function readErrorResponse(
  response: Response,
  fallbackLabel?: string
): Promise<string> {
  const body = await response.text();
  try {
    const error = (JSON.parse(body) as { error?: unknown }).error;
    if (typeof error === "string") return error;
  } catch {
    // Fall through to the caller-selected fallback.
  }
  return fallbackLabel === undefined
    ? body
    : `${fallbackLabel} failed (HTTP ${response.status})`;
}
