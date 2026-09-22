import { afterEach, expect, it, vi } from "vitest";
import { postJson, resetKeepaliveBudgetForTests } from "../src/previewHttp";

afterEach(() => {
  resetKeepaliveBudgetForTests();
});

function deferredResponse(): {
  promise: Promise<Response>;
  resolve(response: Response): void;
  reject(error: unknown): void;
} {
  let resolve!: (response: Response) => void;
  let reject!: (error: unknown) => void;
  return {
    promise: new Promise<Response>((settle, fail) => {
      resolve = settle;
      reject = fail;
    }),
    resolve(response): void {
      resolve(response);
    },
    reject(error): void {
      reject(error);
    }
  };
}

function payloadWithEncodedSize(size: number): { text: string } {
  const emptySize = new TextEncoder().encode(JSON.stringify({ text: "" })).length;
  return { text: "x".repeat(size - emptySize) };
}

it("grants keepalive to a 60,000-byte JSON body", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(
    (_input: RequestInfo | URL, _init?: RequestInit) => pending.promise
  );
  const payload = payloadWithEncodedSize(60_000);

  const request = postJson(
    fetchMock as unknown as typeof fetch,
    "/write",
    payload,
    true
  );

  expect(new TextEncoder().encode(JSON.stringify(payload))).toHaveLength(60_000);
  expect(fetchMock).toHaveBeenCalledWith("/write", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
    keepalive: true
  });
  pending.resolve({} as Response);
  await request;
});

it("budgets concurrent keepalive bodies in aggregate and releases settled bytes", async () => {
  const pending = [deferredResponse(), deferredResponse(), deferredResponse()];
  const fetchMock = vi.fn(
    (_input: RequestInfo | URL, _init?: RequestInit) =>
      pending[fetchMock.mock.calls.length - 1].promise
  );

  const first = postJson(
    fetchMock as unknown as typeof fetch,
    "/first",
    payloadWithEncodedSize(40_000),
    true
  );
  const second = postJson(
    fetchMock as unknown as typeof fetch,
    "/second",
    payloadWithEncodedSize(21_000),
    true
  );
  expect(fetchMock.mock.calls.map(([, init]) => init?.keepalive)).toEqual([true, false]);

  pending[0].resolve({} as Response);
  await first;
  const third = postJson(
    fetchMock as unknown as typeof fetch,
    "/third",
    payloadWithEncodedSize(21_000),
    true
  );
  expect(fetchMock.mock.calls.map(([, init]) => init?.keepalive)).toEqual([
    true,
    false,
    true
  ]);

  pending[1].resolve({} as Response);
  pending[2].resolve({} as Response);
  await Promise.all([second, third]);
});

it("releases rejected keepalive bodies from the aggregate budget", async () => {
  const pending = [deferredResponse(), deferredResponse()];
  const fetchMock = vi.fn(
    (_input: RequestInfo | URL, _init?: RequestInit) =>
      pending[fetchMock.mock.calls.length - 1].promise
  );
  const payload = payloadWithEncodedSize(60_000);

  const rejected = postJson(
    fetchMock as unknown as typeof fetch,
    "/rejected",
    payload,
    true
  );
  expect(fetchMock.mock.calls[0][1]?.keepalive).toBe(true);
  pending[0].reject(new Error("network failed"));
  await expect(rejected).rejects.toThrow("network failed");

  const retry = postJson(
    fetchMock as unknown as typeof fetch,
    "/retry",
    payload,
    true
  );
  expect(fetchMock.mock.calls[1][1]?.keepalive).toBe(true);
  pending[1].resolve({} as Response);
  await retry;
});

it("does not charge the budget when fetch throws synchronously", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(
    (_input: RequestInfo | URL, _init?: RequestInit): Promise<Response> => {
      if (fetchMock.mock.calls.length === 1) throw new Error("synchronous fetch failure");
      return pending.promise;
    }
  );
  const payload = payloadWithEncodedSize(60_000);

  expect(() =>
    postJson(fetchMock as unknown as typeof fetch, "/throws", payload, true)
  ).toThrow("synchronous fetch failure");

  const retry = postJson(
    fetchMock as unknown as typeof fetch,
    "/retry",
    payload,
    true
  );
  expect(fetchMock.mock.calls.map(([, init]) => init?.keepalive)).toEqual([true, true]);
  pending.resolve({} as Response);
  await retry;
});

it("releases each keepalive charge exactly once", async () => {
  const pending = [deferredResponse(), deferredResponse()];
  const doubleFinally = {
    finally(onFinally: () => void): Promise<Response> {
      onFinally();
      onFinally();
      return Promise.resolve({} as Response);
    }
  } as unknown as Promise<Response>;
  const fetchMock = vi.fn(
    (_input: RequestInfo | URL, _init?: RequestInit): Promise<Response> => {
      const call = fetchMock.mock.calls.length;
      if (call === 1) return doubleFinally;
      return pending[call - 2].promise;
    }
  );

  await postJson(
    fetchMock as unknown as typeof fetch,
    "/double-finally",
    payloadWithEncodedSize(40_000),
    true
  );
  const exactLimit = postJson(
    fetchMock as unknown as typeof fetch,
    "/exact-limit",
    payloadWithEncodedSize(60_000),
    true
  );
  expect(fetchMock.mock.calls[1][1]?.keepalive).toBe(true);
  pending[0].resolve({} as Response);
  await exactLimit;

  const overLimit = postJson(
    fetchMock as unknown as typeof fetch,
    "/over-limit",
    payloadWithEncodedSize(60_001),
    true
  );
  expect(fetchMock.mock.calls[2][1]?.keepalive).toBe(false);
  pending[1].resolve({} as Response);
  await overLimit;
});

it("does not charge non-keepalive requests against the budget", async () => {
  const pending = [deferredResponse(), deferredResponse()];
  const fetchMock = vi.fn(
    (_input: RequestInfo | URL, _init?: RequestInit) =>
      pending[fetchMock.mock.calls.length - 1].promise
  );

  const plain = postJson(
    fetchMock as unknown as typeof fetch,
    "/plain",
    payloadWithEncodedSize(59_000),
    false
  );
  const keepalive = postJson(
    fetchMock as unknown as typeof fetch,
    "/keepalive",
    payloadWithEncodedSize(59_000),
    true
  );

  expect(fetchMock.mock.calls.map(([, init]) => init?.keepalive)).toEqual([false, true]);
  pending[0].resolve({} as Response);
  pending[1].resolve({} as Response);
  await Promise.all([plain, keepalive]);
});

it("measures multibyte JSON bodies by UTF-8 bytes", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(
    (_input: RequestInfo | URL, _init?: RequestInit) => pending.promise
  );
  const payload = { text: "界".repeat(20_000) };
  const body = JSON.stringify(payload);
  expect(body.length).toBeLessThan(60_000);
  expect(new TextEncoder().encode(body).length).toBeGreaterThan(60_000);

  const request = postJson(
    fetchMock as unknown as typeof fetch,
    "/write",
    payload,
    true
  );

  expect(fetchMock.mock.calls[0][1]?.keepalive).toBe(false);
  pending.resolve({} as Response);
  await request;
});
