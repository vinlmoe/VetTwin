// Liaison avec le relais de la salle de simulation (salle/relay.py, docs/INTEGRATION_MATERIEL.tex).
// Activée par l'adresse : `?room=1` (relais à la même adresse que la page) ou
// `?relay=ws://hote:8765/ws`, plus `&key=...` si le relais a une clé.
// Usage pédagogique uniquement.

/** Paramètres de salle lus dans l'adresse, ou null si la page n'est pas reliée à une salle. */
export function roomConfig() {
  const q = new URLSearchParams(location.search);
  const relay = q.get('relay');
  if (!relay && q.get('room') !== '1') return null;
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  let url = relay || `${proto}//${location.host}/ws`;
  const key = q.get('key');
  if (key) url += (url.includes('?') ? '&' : '?') + 'key=' + encodeURIComponent(key);
  return { url, key };
}

/**
 * Client WebSocket du relais, reconnecté automatiquement.
 * `onMessage(msg)` reçoit chaque message JSON ; `onStatus({connected, regie})` l'état de la liaison.
 */
export class Room {
  constructor(cfg, role, { onMessage = () => {}, onStatus = () => {} } = {}) {
    this.cfg = cfg; this.role = role; this.onMessage = onMessage; this.onStatus = onStatus;
    this.ws = null; this.connected = false; this.regie = false; this.retry = 500; this.closed = false;
    this.open();
  }

  open() {
    if (this.closed) return;
    let ws;
    try { ws = new WebSocket(this.cfg.url); } catch (e) { this.later(); return; }
    this.ws = ws;
    ws.onopen = () => { this.retry = 500; ws.send(JSON.stringify({ kind: 'hello', role: this.role })); };
    ws.onmessage = (e) => {
      let m; try { m = JSON.parse(e.data); } catch (err) { return; }
      if (m.kind === 'welcome') { this.connected = true; this.regie = !!m.regie || this.role === 'regie'; this.status(); }
      if (m.kind === 'regie') { this.regie = m.connected; this.status(); }
      this.onMessage(m);
    };
    ws.onclose = () => { this.connected = false; this.status(); this.later(); };
    ws.onerror = () => { try { ws.close(); } catch (e) { /* déjà fermé */ } };
  }

  later() {
    if (this.closed) return;
    setTimeout(() => this.open(), this.retry);
    this.retry = Math.min(5000, this.retry * 2);
  }

  status() { this.onStatus({ connected: this.connected, regie: this.regie }); }

  send(obj) {
    if (this.ws && this.ws.readyState === 1) { this.ws.send(JSON.stringify(obj)); return true; }
    return false;
  }

  close() { this.closed = true; if (this.ws) this.ws.close(); }
}
