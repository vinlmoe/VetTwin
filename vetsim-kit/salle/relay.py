#!/usr/bin/env python3
"""Relais de la salle de simulation VetTwins (bibliothèque standard uniquement, Python >= 3.8).

Un seul processus sur le poste serveur (Mac mini, Raspberry Pi, portable) :
- sert l'application web (dossier web/ construit par web/build.sh) ;
- /ws : WebSocket entre la régie (seule à exécuter le moteur), les moniteurs et le lecteur
  d'auscultation ;
- POST /api/device : entrée d'un boîtier (ESP32) ; transmise à la régie, qui la traduit en
  événement du moteur et renvoie l'acceptation ou le refus du moteur (aller-retour <= 3 s) ;
- GET /api/state (JSON) et GET /api/pulse (texte) : dernier état diffusé par la régie, pour
  les boîtiers de sortie (pouls) ;
- GET /api/status : régie connectée, clients.

Le relais ne calcule rien et ne connaît aucun paramètre physiologique : il transmet.
Usage pédagogique uniquement.

    python3 salle/relay.py --web web --host 0.0.0.0 --port 8765 [--key SECRET]
"""
import argparse
import asyncio
import base64
import hashlib
import json
import mimetypes
import os
import struct
import sys
import time
from urllib.parse import parse_qs, urlsplit

WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
PROTOCOL = 1
DEVICE_TIMEOUT_S = 3.0
MAX_BODY = 64 * 1024
MAX_WS_MESSAGE = 8 * 1024 * 1024
MIME = {
    ".wasm": "application/wasm", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8",
    ".json": "application/json; charset=utf-8", ".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8",
    ".svg": "image/svg+xml", ".png": "image/png", ".wav": "audio/wav",
}
# Types d'entrée acceptés des boîtiers (traduits par la régie, web/room.js).
INPUT_TYPES = {
    "inject", "vaporizer", "fresh_gas", "oxygen", "probes", "stethoscope", "procedure_next", "end_surgery",
    "hemorrhage", "fluid_bolus", "stimulus", "intubate", "extubate", "warming", "fluids", "circuit", "ventilation",
}


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


class WsClient:
    def __init__(self, reader, writer, peer):
        self.reader, self.writer, self.peer = reader, writer, peer
        self.role = None
        self.lock = asyncio.Lock()
        self.closed = False

    async def send_text(self, text):
        if self.closed:
            return
        data = text.encode("utf-8")
        n = len(data)
        if n < 126:
            head = struct.pack("!BB", 0x81, n)
        elif n < 65536:
            head = struct.pack("!BBH", 0x81, 126, n)
        else:
            head = struct.pack("!BBQ", 0x81, 127, n)
        try:
            async with self.lock:
                self.writer.write(head + data)
                await self.writer.drain()
        except (ConnectionError, RuntimeError):
            self.closed = True

    async def send(self, obj):
        await self.send_text(json.dumps(obj, ensure_ascii=False, separators=(",", ":")))

    async def _control(self, opcode, payload=b""):
        try:
            async with self.lock:
                self.writer.write(struct.pack("!BB", 0x80 | opcode, len(payload)) + payload)
                await self.writer.drain()
        except (ConnectionError, RuntimeError):
            self.closed = True

    async def recv(self):
        """Message texte suivant (str), ou None à la fermeture."""
        parts = []
        size = 0
        while True:
            b1, b2 = await self.reader.readexactly(2)
            fin, opcode = b1 & 0x80, b1 & 0x0F
            masked, n = b2 & 0x80, b2 & 0x7F
            if n == 126:
                (n,) = struct.unpack("!H", await self.reader.readexactly(2))
            elif n == 127:
                (n,) = struct.unpack("!Q", await self.reader.readexactly(8))
            if n > MAX_WS_MESSAGE:
                raise ValueError("message trop long")
            mask = await self.reader.readexactly(4) if masked else b""
            payload = await self.reader.readexactly(n)
            if masked:
                payload = bytes(c ^ mask[i % 4] for i, c in enumerate(payload))
            if opcode == 0x8:  # fermeture
                await self._control(0x8)
                return None
            if opcode == 0x9:  # ping
                await self._control(0xA, payload[:125])
                continue
            if opcode == 0xA:  # pong
                continue
            if opcode in (0x1, 0x0, 0x2):
                parts.append(payload)
                size += n
                if size > MAX_WS_MESSAGE:
                    raise ValueError("message trop long")
                if fin:
                    return b"".join(parts).decode("utf-8", errors="replace")


class Relay:
    def __init__(self, web_dir, key=None):
        self.web_dir = os.path.realpath(web_dir) if web_dir else None
        self.key = key
        self.clients = set()
        self.regie = None
        self.cache = {}  # dernier message diffusé par la régie, par type (snapshot, waves, steth, probes, session)
        self.cache_t = {}
        self.pending = {}
        self.next_req = 1

    # ---------------------------------------------------------------- HTTP
    async def handle(self, reader, writer):
        peer = writer.get_extra_info("peername")
        try:
            head = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), timeout=10)
        except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, asyncio.TimeoutError, ConnectionError):
            writer.close()
            return
        lines = head.decode("latin-1").split("\r\n")
        try:
            method, target, _ = lines[0].split(" ", 2)
        except ValueError:
            writer.close()
            return
        headers = {}
        for ln in lines[1:]:
            if ":" in ln:
                k, v = ln.split(":", 1)
                headers[k.strip().lower()] = v.strip()
        url = urlsplit(target)
        query = parse_qs(url.query)
        path = url.path
        try:
            if path == "/ws" and headers.get("upgrade", "").lower() == "websocket":
                if not self.authorized(headers, query):
                    await self.respond(writer, 403, {"ok": False, "error": "clé du relais invalide"})
                    return
                await self.websocket(reader, writer, headers, peer)
                return
            body = b""
            n = int(headers.get("content-length", "0") or 0)
            if n > MAX_BODY:
                await self.respond(writer, 413, {"ok": False, "error": "corps trop long"})
                return
            if n:
                body = await asyncio.wait_for(reader.readexactly(n), timeout=10)
            if path.startswith("/api/"):
                if method == "OPTIONS":
                    await self.respond(writer, 204, None)
                    return
                if not self.authorized(headers, query):
                    await self.respond(writer, 403, {"ok": False, "error": "clé du relais invalide"})
                    return
                await self.api(writer, method, path, body)
                return
            if method not in ("GET", "HEAD"):
                await self.respond(writer, 405, {"ok": False, "error": "méthode non permise"})
                return
            await self.static(writer, path, method == "HEAD")
        except (ConnectionError, asyncio.IncompleteReadError, asyncio.TimeoutError):
            pass
        finally:
            if not writer.is_closing():
                try:
                    writer.close()
                except RuntimeError:
                    pass

    def authorized(self, headers, query):
        if not self.key:
            return True
        return headers.get("x-vettwin-key") == self.key or query.get("key", [None])[0] == self.key

    async def respond(self, writer, code, obj, ctype="application/json; charset=utf-8", raw=None, head_only=False):
        reason = {200: "OK", 204: "No Content", 400: "Bad Request", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed",
                  413: "Payload Too Large", 503: "Service Unavailable", 504: "Gateway Timeout"}.get(code, "OK")
        data = raw if raw is not None else (b"" if obj is None else json.dumps(obj, ensure_ascii=False).encode("utf-8"))
        hdr = [f"HTTP/1.1 {code} {reason}", f"Content-Length: {len(data)}", "Connection: close", "Cache-Control: no-cache",
               "Access-Control-Allow-Origin: *", "Access-Control-Allow-Headers: Content-Type, X-VetTwin-Key",
               "Access-Control-Allow-Methods: GET, POST, OPTIONS"]
        if data:
            hdr.append(f"Content-Type: {ctype}")
        writer.write(("\r\n".join(hdr) + "\r\n\r\n").encode("latin-1") + (b"" if head_only else data))
        await writer.drain()

    async def static(self, writer, path, head_only):
        if not self.web_dir:
            await self.respond(writer, 404, {"ok": False, "error": "aucun dossier web servi"})
            return
        rel = path.lstrip("/") or "index.html"
        full = os.path.realpath(os.path.join(self.web_dir, rel))
        if os.path.isdir(full):
            full = os.path.join(full, "index.html")
        if not (full == self.web_dir or full.startswith(self.web_dir + os.sep)) or not os.path.isfile(full):
            await self.respond(writer, 404, {"ok": False, "error": "introuvable"})
            return
        ext = os.path.splitext(full)[1].lower()
        ctype = MIME.get(ext) or mimetypes.guess_type(full)[0] or "application/octet-stream"
        with open(full, "rb") as f:
            data = f.read()
        await self.respond(writer, 200, None, ctype=ctype, raw=data, head_only=head_only)

    async def api(self, writer, method, path, body):
        if path == "/api/status" and method == "GET":
            roles = {}
            for c in self.clients:
                roles[c.role or "?"] = roles.get(c.role or "?", 0) + 1
            await self.respond(writer, 200, {"ok": True, "protocol": PROTOCOL, "regie": self.regie is not None, "clients": roles,
                                             "last_snapshot_age_s": self.age("snapshot")})
            return
        if path == "/api/state" and method == "GET":
            snap = self.cache.get("snapshot")
            if not snap:
                await self.respond(writer, 503, {"ok": False, "error": "aucun état diffusé par la régie"})
                return
            await self.respond(writer, 200, {"ok": True, "age_s": self.age("snapshot"), "regie": self.regie is not None,
                                             "pulse": snap.get("pulse"), "probes": snap.get("probes"), "t_s": snap.get("t_s")})
            return
        if path == "/api/pulse" and method == "GET":
            # Texte d'une ligne pour un microcontrôleur : "fc pas pad décès valide âge_s".
            # valide = 1 si la régie est connectée et l'état a moins de 5 s (elle en renvoie un
            # toutes les 2 s, même en pause) ; sinon le boîtier doit se taire.
            snap = self.cache.get("snapshot") or {}
            p = snap.get("pulse") or {}
            age = self.age("snapshot")
            stale = age is None or age > 5 or self.regie is None
            num = lambda x: f"{x:.1f}" if isinstance(x, (int, float)) else "0"
            line = " ".join([num(p.get("hr")), num(p.get("sap")), num(p.get("dap")), "1" if p.get("dead") else "0",
                             "0" if stale else "1", num(age if age is not None else 999)])
            await self.respond(writer, 200, None, ctype="text/plain; charset=utf-8", raw=(line + "\n").encode())
            return
        if path == "/api/device" and method == "POST":
            try:
                msg = json.loads(body.decode("utf-8"))
                device = str(msg["device"])[:64]
                inp = msg["input"]
                if not isinstance(inp, dict) or inp.get("type") not in INPUT_TYPES:
                    raise ValueError
            except (ValueError, KeyError, TypeError, UnicodeDecodeError):
                await self.respond(writer, 400, {"ok": False, "error": "attendu {device, input: {type, ...}} ; types : " + ", ".join(sorted(INPUT_TYPES))})
                return
            code, res = await self.forward(device, inp)
            await self.respond(writer, code, res)
            return
        await self.respond(writer, 404, {"ok": False, "error": "point d'API inconnu"})

    def age(self, kind):
        t = self.cache_t.get(kind)
        return None if t is None else round(time.monotonic() - t, 2)

    async def forward(self, device, inp):
        """Entrée d'un boîtier → régie ; attend son verdict (moteur)."""
        if self.regie is None:
            return 503, {"ok": False, "error": "régie non connectée"}
        req = self.next_req
        self.next_req += 1
        fut = asyncio.get_running_loop().create_future()
        self.pending[req] = fut
        await self.regie.send({"kind": "device", "req": req, "device": device, "input": inp})
        try:
            res = await asyncio.wait_for(fut, timeout=DEVICE_TIMEOUT_S)
        except asyncio.TimeoutError:
            return 504, {"ok": False, "error": "pas de réponse de la régie"}
        finally:
            self.pending.pop(req, None)
        log(f"{device} {inp.get('type')} -> {'accepté' if res.get('ok') else 'refusé'} {res.get('text', '')}")
        return 200, {"ok": bool(res.get("ok")), "text": str(res.get("text", ""))[:200]}

    # ---------------------------------------------------------------- WebSocket
    async def websocket(self, reader, writer, headers, peer):
        key = headers.get("sec-websocket-key")
        if not key:
            await self.respond(writer, 400, {"ok": False, "error": "Sec-WebSocket-Key manquant"})
            return
        accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
        writer.write(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                      f"Sec-WebSocket-Accept: {accept}\r\n\r\n").encode())
        await writer.drain()
        c = WsClient(reader, writer, peer)
        self.clients.add(c)
        try:
            while True:
                text = await c.recv()
                if text is None:
                    break
                try:
                    msg = json.loads(text)
                except ValueError:
                    continue
                if isinstance(msg, dict):
                    await self.on_message(c, msg)
        except (asyncio.IncompleteReadError, ConnectionError, ValueError):
            pass
        finally:
            c.closed = True
            self.clients.discard(c)
            if self.regie is c:
                self.regie = None
                log("régie déconnectée")
                await self.broadcast({"kind": "regie", "connected": False}, exclude=c)
            writer.close()

    async def on_message(self, c, msg):
        kind = msg.get("kind")
        if kind == "hello":
            role = str(msg.get("role", "viewer"))[:16]
            c.role = role
            if role == "regie":
                old = self.regie
                self.regie = c
                if old is not None and old is not c:
                    await old.send({"kind": "replaced", "text": "une autre régie s'est connectée"})
                log(f"régie connectée ({c.peer[0] if c.peer else '?'})")
                await self.broadcast({"kind": "regie", "connected": True}, exclude=c)
            else:
                log(f"{role} connecté ({c.peer[0] if c.peer else '?'})")
            await c.send({"kind": "welcome", "protocol": PROTOCOL, "role": role, "regie": self.regie is not None})
            if role != "regie":
                for k in ("session", "probes", "steth", "snapshot", "waves"):
                    if k in self.cache:
                        await c.send(self.cache[k])
            return
        if c is self.regie:
            if kind == "result":
                fut = self.pending.get(msg.get("req"))
                if fut and not fut.done():
                    fut.set_result(msg)
                return
            if kind in ("snapshot", "waves", "steth", "probes", "session", "notice"):
                if kind != "notice":
                    self.cache[kind] = msg
                    self.cache_t[kind] = time.monotonic()
                await self.broadcast(msg, exclude=c)
            return
        # Boîtier relié en WebSocket (au lieu de POST) : même traitement.
        if kind == "device" and isinstance(msg.get("input"), dict) and msg["input"].get("type") in INPUT_TYPES:
            _, res = await self.forward(str(msg.get("device", c.role))[:64], msg["input"])
            await c.send({"kind": "device_result", "req": msg.get("req"), **res})

    async def broadcast(self, msg, exclude=None):
        text = json.dumps(msg, ensure_ascii=False, separators=(",", ":"))
        await asyncio.gather(*(c.send_text(text) for c in list(self.clients) if c is not exclude and c.role), return_exceptions=True)


async def main(argv=None):
    ap = argparse.ArgumentParser(description="Relais de la salle de simulation VetTwins")
    ap.add_argument("--web", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "web"), help="dossier web/ construit")
    ap.add_argument("--host", default="127.0.0.1", help="adresse d'écoute (0.0.0.0 = toutes les interfaces du réseau de la salle)")
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--key", default=os.environ.get("VETTWIN_KEY"), help="clé partagée (en-tête X-VetTwin-Key ou ?key=)")
    a = ap.parse_args(argv)
    relay = Relay(a.web, a.key)
    server = await asyncio.start_server(relay.handle, a.host, a.port, limit=MAX_BODY)
    log(f"relais VetTwins sur http://{a.host}:{a.port}/ (web : {relay.web_dir}, clé : {'oui' if a.key else 'non'})")
    if relay.web_dir and not os.path.isdir(os.path.join(relay.web_dir, "pkg")):
        log("attention : web/pkg absent, lancer d'abord sh web/build.sh")
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(0)
