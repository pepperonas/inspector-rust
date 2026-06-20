// Inspector Rust — Timesheet Bridge (MV3 service worker).
//
// Reports the active tab to the desktop app's loopback WebSocket
// (ws://127.0.0.1:<port>) so the timesheet can attribute browser time to a
// host/title. The DESKTOP is the clock — this only sends URL/title; it never
// records time itself. Nothing is sent anywhere except 127.0.0.1.
//
// Cross-browser: uses `browser` (Firefox) or `chrome` (Chromium). The service
// worker can be suspended; a short keep-alive alarm + lazy reconnect keep the
// socket healthy and re-report the active tab.

const api = globalThis.browser ?? globalThis.chrome;

let ws = null;

async function cfg() {
  const d = await api.storage.local.get(["port", "token", "hostOnly"]);
  return {
    port: Number(d.port) || 47823,
    token: d.token || "",
    hostOnly: !!d.hostOnly,
  };
}

function socketOpenOrConnecting() {
  return ws && (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN);
}

function connect(c) {
  if (socketOpenOrConnecting()) return;
  if (!c.token) return; // not configured yet
  try {
    ws = new WebSocket(`ws://127.0.0.1:${c.port}`);
  } catch {
    ws = null;
    return;
  }
  ws.onopen = () => {
    try {
      ws.send(JSON.stringify({ type: "hello", token: c.token }));
    } catch {
      /* ignore */
    }
    reportActive(c);
  };
  ws.onclose = () => {
    ws = null;
  };
  ws.onerror = () => {
    /* onclose handles cleanup */
  };
}

async function ensure() {
  const c = await cfg();
  if (!socketOpenOrConnecting()) connect(c);
  return c;
}

async function reportActive(known) {
  const c = known ?? (await cfg());
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  let tabs;
  try {
    tabs = await api.tabs.query({ active: true, lastFocusedWindow: true });
  } catch {
    return;
  }
  const t = tabs && tabs[0];
  if (!t || !t.url) return;
  let url = t.url;
  if (c.hostOnly) {
    try {
      url = new URL(t.url).origin;
    } catch {
      /* keep full url */
    }
  }
  try {
    ws.send(JSON.stringify({ type: "tab", url, title: t.title || "", ts: Date.now() }));
  } catch {
    /* dropped — next event reconnects */
  }
}

function sendBlur() {
  if (ws && ws.readyState === WebSocket.OPEN) {
    try {
      ws.send(JSON.stringify({ type: "blur" }));
    } catch {
      /* ignore */
    }
  }
}

api.tabs.onActivated.addListener(async () => {
  const c = await ensure();
  reportActive(c);
});

api.tabs.onUpdated.addListener(async (_id, info, tab) => {
  if (info.status === "complete" && tab && tab.active) {
    const c = await ensure();
    reportActive(c);
  }
});

api.windows.onFocusChanged.addListener(async (winId) => {
  if (winId === api.windows.WINDOW_ID_NONE) {
    sendBlur();
  } else {
    const c = await ensure();
    reportActive(c);
  }
});

// Keep-alive: MV3 workers idle out; a ~24 s alarm wakes us to reconnect + re-report.
api.alarms.create("keepalive", { periodInMinutes: 0.4 });
api.alarms.onAlarm.addListener(async () => {
  const c = await ensure();
  reportActive(c);
});

ensure();
