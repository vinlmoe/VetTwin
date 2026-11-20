#!/usr/bin/env python3
"""Test du relais de salle (salle/relay.py), bibliothèque standard uniquement.

Lance le relais sur un port libre, simule une régie (WebSocket), un moniteur et un boîtier
(HTTP POST), et vérifie : aller-retour boîtier → régie → verdict, refus sans régie, délai,
diffusion des instantanés, /api/pulse, fichiers statiques (type wasm, sortie du dossier
refusée), clé partagée. Chaque contrôle peut échouer pour la raison qu'il teste.

    python tests/check_relay.py
"""
import asyncio
import base64
import json
import os
import socket
import struct
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
RELAY = os.path.join(HERE, "..", "salle", "relay.py")
FAIL = []


def check(cond, what):
    print(("ok   " if cond else "ÉCHEC ") + what)
    if not cond:
        FAIL.append(what)


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def http(port, path, body=None, key=None, method=None):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=None if body is None else json.dumps(body).encode(),
                                 method=method or ("POST" if body is not None else "GET"))
    if body is not None:
        req.add_header("Content-Type", "application/json")
    if key:
        req.add_header("X-VetTwin-Key", key)
    try:
        with urllib.request.urlopen(req, timeout=6) as r:
            return r.status, r.headers.get("Content-Type", ""), r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.headers.get("Content-Type", ""), e.read()


class Ws:
    """Client WebSocket minimal (trames masquées, comme un navigateur)."""

    @classmethod
    async def open(cls, port, role, key=None):
        self = cls()
        self.r, self.w = await asyncio.open_connection("127.0.0.1", port)
        k = base64.b64encode(os.urandom(16)).decode()
        q = f"?key={key}" if key else ""
        self.w.write((f"GET /ws{q} HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                      f"Sec-WebSocket-Key: {k}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
        await self.w.drain()
        head = await self.r.readuntil(b"\r\n\r\n")
        self.status = int(head.split(b" ")[1])
        if self.status == 101:
            await self.send({"kind": "hello", "role": role})
        return self

    async def send(self, obj):
        data = json.dumps(obj).encode()
        mask = os.urandom(4)
        n = len(data)
        head = struct.pack("!BB", 0x81, 0x80 | n) if n < 126 else struct.pack("!BBH", 0x81, 0x80 | 126, n) if n < 65536 else struct.pack("!BBQ", 0x81, 0x80 | 127, n)
        self.w.write(head + mask + bytes(c ^ mask[i % 4] for i, c in enumerate(data)))
        await self.w.drain()

    async def recv(self, timeout=3):
        async def one():
            b1, b2 = await self.r.readexactly(2)
            n = b2 & 0x7F
            if n == 126:
                (n,) = struct.unpack("!H", await self.r.readexactly(2))
            elif n == 127:
                (n,) = struct.unpack("!Q", await self.r.readexactly(8))
            return json.loads((await self.r.readexactly(n)).decode())
        return await asyncio.wait_for(one(), timeout)

    async def until(self, kind, timeout=3):
        t0 = time.monotonic()
        while True:
            m = await self.recv(max(0.1, timeout - (time.monotonic() - t0)))
            if m.get("kind") == kind:
                return m


async def scenario(port, key=None):
    loop = asyncio.get_running_loop()
    post = lambda body, k=key: loop.run_in_executor(None, http, port, "/api/device", body, k)

    # sans régie : refus explicite (503)
    code, _, body = await post({"device": "t", "input": {"type": "probes", "ecg": True}})
    check(code == 503 and not json.loads(body)["ok"], "boîtier sans régie : 503")

    regie = await Ws.open(port, "regie", key)
    check(regie.status == 101, "régie : poignée de main WebSocket")
    w = await regie.until("welcome")
    check(w.get("role") == "regie", "régie : accueil")
    mon = await Ws.open(port, "monitor", key)
    await mon.until("welcome")

    # aller-retour : la régie accepte ou refuse, le boîtier reçoit le verdict
    async def regie_answers(n):
        for _ in range(n):
            m = await regie.until("device")
            ok = m["input"].get("type") != "vaporizer"
            await regie.send({"kind": "result", "req": m["req"], "ok": ok, "text": "accepté" if ok else "refusé par le moteur"})
    task = asyncio.ensure_future(regie_answers(2))
    code, _, body = await post({"device": "pupitre-1", "input": {"type": "inject", "agent": "propofol", "conc_mg_ml": 10, "ml": 2}})
    r = json.loads(body)
    check(code == 200 and r["ok"] and r["text"] == "accepté", "boîtier → régie → verdict accepté")
    code, _, body = await post({"device": "machine", "input": {"type": "vaporizer", "agent": "isoflurane", "pct": 99}})
    r = json.loads(body)
    check(code == 200 and not r["ok"] and "refusé" in r["text"], "boîtier → régie → verdict refusé transmis")
    await task

    # type d'entrée inconnu : refusé par le relais sans déranger la régie
    code, _, _ = await post({"device": "x", "input": {"type": "rm -rf"}})
    check(code == 400, "type d'entrée inconnu : 400")
    code, _, _ = await loop.run_in_executor(None, http, port, "/api/device", None, key, "POST")
    check(code == 400, "corps vide : 400")

    # régie muette : délai dépassé (504)
    t0 = time.monotonic()
    code, _, _ = await post({"device": "t", "input": {"type": "probes", "ecg": False}})
    check(code == 504 and 2.5 < time.monotonic() - t0 < 5, "régie muette : 504 après ~3 s")
    await regie.until("device")  # message resté sans réponse

    # instantané diffusé au moniteur, mis en cache, lisible par /api/pulse
    snap = {"kind": "snapshot", "t_s": 125.0, "pulse": {"hr": 88.4, "sap": 112.0, "dap": 61.0, "map": 78.0, "dead": False, "running": True},
            "probes": {"ecg": True}}
    await regie.send(snap)
    m = await mon.until("snapshot")
    check(m["pulse"]["hr"] == 88.4, "instantané diffusé au moniteur")
    await asyncio.sleep(0.1)
    code, ctype, body = await loop.run_in_executor(None, http, port, "/api/pulse", None, key)
    parts = body.decode().split()
    check(code == 200 and ctype.startswith("text/plain") and parts[:5] == ["88.4", "112.0", "61.0", "0", "1"], f"/api/pulse : {body.decode().strip()}")
    code, _, body = await loop.run_in_executor(None, http, port, "/api/state", None, key)
    check(code == 200 and json.loads(body)["pulse"]["sap"] == 112.0, "/api/state")

    # un moniteur arrivé après coup reçoit le dernier instantané
    late = await Ws.open(port, "monitor", key)
    m = await late.until("snapshot")
    check(m["t_s"] == 125.0, "moniteur tardif : dernier instantané renvoyé")

    # une seconde régie remplace la première, qui est prévenue
    regie2 = await Ws.open(port, "regie", key)
    m = await regie.until("replaced")
    check(m["kind"] == "replaced", "seconde régie : la première est prévenue")

    # déconnexion de la régie : /api/pulse passe à « pas en lecture »
    for c in (regie, regie2):
        c.w.close()
    await asyncio.sleep(0.3)
    code, _, body = await loop.run_in_executor(None, http, port, "/api/pulse", None, key)
    check(body.decode().split()[4] == "0", "régie partie : pouls arrêté (lecture = 0)")
    code, _, body = await loop.run_in_executor(None, http, port, "/api/status", None, key)
    check(json.loads(body)["regie"] is False, "/api/status : régie absente")
    for c in (mon, late):
        c.w.close()


def run(args, web):
    port = free_port()
    p = subprocess.Popen([sys.executable, "-I", RELAY, "--web", web, "--port", str(port)] + args,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    for _ in range(50):
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
            break
        except OSError:
            time.sleep(0.1)
    return p, port


def main():
    with tempfile.TemporaryDirectory() as tmp:
        web = os.path.join(tmp, "web")
        os.makedirs(os.path.join(web, "pkg"))
        open(os.path.join(web, "index.html"), "w").write("<!doctype html>ok")
        open(os.path.join(web, "pkg", "m.wasm"), "wb").write(b"\0asm\1\0\0\0")
        open(os.path.join(tmp, "secret.txt"), "w").write("secret")

        p, port = run([], web)
        try:
            code, ctype, body = http(port, "/")
            check(code == 200 and body.startswith(b"<!doctype"), "fichier statique : index.html")
            code, ctype, _ = http(port, "/pkg/m.wasm")
            check(code == 200 and ctype == "application/wasm", "type MIME application/wasm")
            code, _, body = http(port, "/../secret.txt")
            check(code == 404 and b"secret" not in body, "sortie du dossier web refusée")
            code, _, body = http(port, "/pkg/%2e%2e/%2e%2e/secret.txt")
            check(code == 404 and b"secret" not in body, "sortie du dossier web refusée (encodée)")
            asyncio.run(scenario(port))
        finally:
            p.terminate()
            out = p.communicate(timeout=5)[0]
        check("Traceback" not in out, "relais sans exception")
        if "Traceback" in out:
            print(out)

        p, port = run(["--key", "s3cret"], web)
        try:
            code, _, _ = http(port, "/api/status")
            check(code == 403, "clé : API refusée sans clé")
            code, _, _ = http(port, "/api/status", key="faux")
            check(code == 403, "clé : API refusée avec une mauvaise clé")
            code, _, _ = http(port, "/api/status", key="s3cret")
            check(code == 200, "clé : API acceptée avec la clé")

            async def ws_nokey():
                c = await Ws.open(port, "regie")
                check(c.status == 403, "clé : WebSocket refusé sans clé")
            asyncio.run(ws_nokey())
            asyncio.run(scenario(port, key="s3cret"))
        finally:
            p.terminate()
            p.communicate(timeout=5)
    print(f"\n{len(FAIL)} échec(s)" if FAIL else "\nrelais : tous les contrôles passent")
    sys.exit(1 if FAIL else 0)


if __name__ == "__main__":
    main()
