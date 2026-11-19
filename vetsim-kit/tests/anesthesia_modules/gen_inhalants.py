# Génère modules/anesthesia/inhalants/*.json.
#   python tests/anesthesia_modules/gen_inhalants.py modules/anesthesia/inhalants tests/anesthesia_modules/coefficients.json
import json, sys
OUT = sys.argv[1]; K = json.load(open(sys.argv[2])) if len(sys.argv) > 2 else {}
k = lambda n, d: K.get(n, d)
def floor_lin(param, c, lo=0.1):
    return {"param": param, "op": "mul", "expr": {"max": [lo, {"sub": [1, {"mul": [c, "M"]}]}]}}
def powl(param, c, p, lo):
    return {"param": param, "op": "mul", "expr": {"max": [lo, {"sub": [1, {"mul": [c, {"pow": ["M", p]}]}]}]}}
def vent(a, n):
    return {"param": "vent_drive", "op": "mul", "expr": {"max": [0, {"sub": [1, {"mul": [a, {"pow": ["M", n]}]}]}]}}
REFS = ["Kazama T, Ikeda K 1988, Anesthesiology 68:435, doi:10.1097/00000542-198803000-00020 (résumé)",
        "Hofmeister EH et al. 2008, Am J Vet Res 69:451, doi:10.2460/ajvr.69.4.451 (résumé)",
        "Soares JH et al. 2012, Anesthesiology, doi:10.1097/aln.0b013e3182557cc9 (PMC3381048, lu)",
        "Yasuda N et al. 1989, Anesth Analg, doi:10.1213/00000539-198909000-00016 (résumé)",
        "Bernard JM et al. 1990, Anesthesiology, doi:10.1097/00000542-199004000-00014 (résumé)",
        "Hoka S et al. 1995, J Anesth, doi:10.1007/bf02479873 (résumé)",
        "Galloway DS et al. 2004, JAVMA 225:700, doi:10.2460/javma.2004.225.700 (résumé)",
        "Steffey EP et al. 1993, Anesth Analg, doi:10.1213/00000539-199377020-00023 (résumé)",
        "Lopez LA et al. 2009, Am J Vet Res, doi:10.2460/ajvr.70.11.1339 (résumé)"]
def mod(id, name, mac, bg, brain, musc, fat, vmax, ees, rsys, va, vn, notes, extra_refs, rp=1.0, baro=0.0):
    return {"schema_version": "1.0.0", "species": ["dog"], "license": "CC-BY-4.0", "id": id, "version": "1.0.0", "name": {"fr": name},
            "evidence": "C", "mac_pct": mac, "blood_gas": bg, "tissue_blood": {"vrg": brain, "muscle": musc, "fat": fat},
            "vaporizer_max_pct": vmax,
            "effects": [floor_lin("ees_lv", ees), floor_lin("ees_rv", ees), powl("r_sys", rsys, rp, 0.1), vent(va, vn), powl("baro_gain", baro, 1.0, 0.0)],
            "refs": REFS + extra_refs, "notes": notes}
iso = mod("isoflurane", "Isoflurane", 1.3, 1.40, 1.57, 2.9, 45.0, 5.0, 0.235, k("iso_rsys", 0.2), k("iso_va", 0.3), k("iso_vn", 1.5),
  "MAC 1,3 % (1,28 à 1,39 % : Steffey & Howland 1977, Kazama 1988, Hofmeister 2008, résumés). Sang/gaz canin 1,40 (Soares 2012, lu). Cerveau/sang 1,57 (humain, Yasuda 1989) ; muscle/sang 2,9 et graisse/sang 45 : ordres de grandeur humains classiques, NON LUS (D). Effets en fonction de M = pression artérielle / MAC : Ees VG et VD −23,5 % par MAC (Hoka 1995 : −23 % à 1 MAC, −48 % à 2 MAC) ; vasodilatation r_sys = max(0,1 ; 1 − c·M^p) et émoussement du baroréflexe baro_gain × (1 − b·M) calés par session sur le sévoflurane de Bernard 1990 (PAM −22 % à 1,2 MAC, −38 % à 2 MAC, DC −17 % à 2 MAC ; isoflurane « similaire »). La dépression du baroréflexe par les halogénés est le mécanisme retenu (non chiffrée dans les sources lues : forme D) ; commande ventilatoire max(0, 1 − a·M^n) calée pour PaCO2 46 mmHg à 1 MAC en ventilation spontanée (Steffey 1993) et apnée à 2,6 MAC (Galloway 2004). Baroréflexe non déprimé par l'agent (placeholder : effet non sourcé).", ["Steffey EP, Howland D 1977, Am J Vet Res 38:1833 (résumé)"], k("iso_rsys_p", 1.0), k("iso_baro", 0.2))
sevo = mod("sevoflurane", "Sévoflurane", 2.2, 0.664, 1.70, 3.1, 48.0, 8.0, 0.2, k("sevo_rsys", 0.2), k("sevo_va", 0.2), k("sevo_vn", 1.5),
  "MAC 2,2 % (2,10 à 2,36 % : Kazama 1988, Hofmeister 2008, Yamashita 2012, Murahata 2018). Sang/gaz canin 0,664 (Soares 2012, lu). Cerveau/sang 1,70 (humain, Yasuda 1989) ; muscle/sang 3,1 et graisse/sang 48 : ordres de grandeur humains NON LUS (D). Ees −20 % par MAC (Hoka 1995 : −16 % à 1 MAC, −41 % à 2 MAC) ; vasodilatation max(0,1 ; 1 − c·M^p) et émoussement du baroréflexe (1 − b·M, mécanisme retenu, forme D) calés sur Bernard 1990 (PAM −22 % à 1,2 MAC, −38 % à 2 MAC, DC −17 % à 2 MAC) ; commande ventilatoire calée pour l'apnée à 3,45 MAC (Galloway 2004), avec la même PaCO2 à 1 MAC que l'isoflurane (hypothèse D, aucune valeur canine publiée trouvée).", ["Yamashita K et al. 2012, J Vet Med Sci, doi:10.1292/jvms.11-0274 (résumé)", "Murahata Y et al. 2018, J Vet Med Sci (PMC6068302, lu)"], k("sevo_rsys_p", 1.0), k("sevo_baro", 0.2))
for m in (iso, sevo):
    json.dump(m, open(f"{OUT}/{m['id']}.json", "w"), ensure_ascii=False, indent=1); open(f"{OUT}/{m['id']}.json", "a").write("\n")
