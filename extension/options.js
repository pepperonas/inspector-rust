const api = globalThis.browser ?? globalThis.chrome;

const portEl = document.getElementById("port");
const tokenEl = document.getElementById("token");
const hostOnlyEl = document.getElementById("hostOnly");
const statusEl = document.getElementById("status");

async function load() {
  const d = await api.storage.local.get(["port", "token", "hostOnly"]);
  if (d.port) portEl.value = d.port;
  if (d.token) tokenEl.value = d.token;
  hostOnlyEl.checked = !!d.hostOnly;
}

document.getElementById("save").addEventListener("click", async () => {
  await api.storage.local.set({
    port: Number(portEl.value) || 47823,
    token: tokenEl.value.trim(),
    hostOnly: hostOnlyEl.checked,
  });
  statusEl.textContent = "Saved ✓";
  setTimeout(() => (statusEl.textContent = ""), 1500);
});

load();
