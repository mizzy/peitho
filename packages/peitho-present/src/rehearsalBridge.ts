import type { RehearsalReportDetail } from "./rehearsalReporter";

export function installRehearsalBridge(
  win: Window,
  bus: EventTarget = win,
  fetcher: typeof fetch = win.fetch.bind(win)
): () => void {
  function onReport(event: Event): void {
    const detail = (event as CustomEvent<RehearsalReportDetail>).detail;
    void fetcher("/rehearsal", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      ...(detail.final ? { keepalive: true } : {}),
      body: JSON.stringify(detail.snapshot)
    })
      .then((response) => {
        if (!response.ok) {
          console.error(`failed to POST rehearsal snapshot: ${response.status}`);
        }
      })
      .catch((error) => {
        console.error("failed to POST rehearsal snapshot", error);
      });
  }

  bus.addEventListener("peitho:rehearsalreport", onReport);

  return () => {
    bus.removeEventListener("peitho:rehearsalreport", onReport);
  };
}
