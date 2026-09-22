// BFCache restores reload the preview module realm, so unload requests need not release this counter.
let inflightKeepaliveBytes = 0;

export function resetKeepaliveBudgetForTests(): void {
  inflightKeepaliveBytes = 0;
}

export function postJson(
  fetcher: typeof fetch,
  url: string,
  payload: unknown,
  keepalive: boolean
): Promise<Response> {
  const body = JSON.stringify(payload);
  const bodyBytes = new TextEncoder().encode(body).length;
  // Chrome rejects when in-flight keepalive bodies exceed 64 KiB in aggregate.
  const useKeepalive = keepalive && inflightKeepaliveBytes + bodyBytes <= 60_000;
  const init: RequestInit = {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body,
    keepalive: useKeepalive
  };
  if (!useKeepalive) return fetcher(url, init);

  let charged = false;
  let released = false;
  const release = (): void => {
    if (!charged || released) return;
    released = true;
    inflightKeepaliveBytes -= bodyBytes;
  };
  try {
    const request = fetcher(url, init);
    inflightKeepaliveBytes += bodyBytes;
    charged = true;
    return request.finally(release);
  } catch (error) {
    release();
    throw error;
  }
}

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
