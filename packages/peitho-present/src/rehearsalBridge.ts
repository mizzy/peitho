import type { RehearsalReportDetail } from "./rehearsalReporter";

export function installRehearsalBridge(
  win: Window,
  bus: EventTarget = win,
  fetcher: typeof fetch = win.fetch.bind(win)
): () => void {
  function onReport(event: Event): void {
    const detail = (event as CustomEvent<RehearsalReportDetail>).detail;
    let request: Promise<Response>;
    try {
      request = fetcher("/rehearsal", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        ...(detail.keepalive ? { keepalive: true } : {}),
        body: JSON.stringify(detail.snapshot)
      });
    } catch (error: unknown) {
      request = Promise.reject(error);
    }
    const completion = request
      .then((response) => {
        if (!response.ok) {
          console.error(`failed to POST rehearsal snapshot: ${response.status}`);
        }
      })
      .catch((error) => {
        console.error("failed to POST rehearsal snapshot", error);
      });
    if (detail.final) detail.waitUntil?.(completion);
  }

  bus.addEventListener("peitho:rehearsalreport", onReport);

  return () => {
    bus.removeEventListener("peitho:rehearsalreport", onReport);
  };
}
