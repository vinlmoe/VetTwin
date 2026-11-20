# Génère modules/anesthesia/agents/*.json (valeurs : docs/ANESTHESIE_SOURCES.md ; coefficients calés :
# coefficients.json, banc crates/vetsim-api/examples/anesthesia_calibrate.rs).
#   python tests/anesthesia_modules/gen_agents.py modules/anesthesia/agents tests/anesthesia_modules/coefficients.json
import json, sys, os
OUT = sys.argv[1]
K = json.load(open(sys.argv[2])) if len(sys.argv) > 2 else {}   # coefficients calibrés
def k(name, default): return K.get(name, default)
def sat(mx, ec, h): return {"max": mx, "ec50_mg_L": ec, "hill": h}
def lin(param, op, c, var="E", clamp=None):
    e = {"param": param, "op": op, "expr": ({"add": [1, {"mul": [c, var]}]} if op == "mul" else {"mul": [c, var]})}
    if clamp: e["clamp"] = clamp
    return e
def apnea(a):
    """Commande ventilatoire abolissable par le pic plasmatique : max(0, 1 − a·EP)."""
    return {"param": "vent_drive", "op": "mul", "expr": {"max": [0, {"sub": [1, {"mul": [a, "EP"]}]}]}}
def mod(id, name, ev, pk, pd, an, refs, notes):
    return {"schema_version": "1.0.0", "species": ["dog"], "license": "CC-BY-4.0", "id": id, "version": "1.0.0",
            "name": {"fr": name}, "evidence": ev, "pk": pk, "pd": pd, "anesthesia": an, "refs": refs, "notes": notes}
def pk(vc, vp, cl, q, renal=0.0, hep=1.0):
    return {"vc_L_kg": vc, "vp_L_kg": vp, "cl_mL_min_kg": cl, "q_mL_min_kg": q, "ka_per_h": 1.0, "f": 0.0, "renal_exp": renal, "hep_exp": hep}
OPIOID_EFFECTS = [lin("vent_drive", "mul", -0.25), lin("vagal_add", "add", 0.27), lin("r_sys", "mul", 0.36)]
M = []
M.append(mod("dexmedetomidine", "Dexmédétomidine", "C",
  pk(0.12, 0.25, 8.0, 30.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 0.002, "hill": 1.0, "effects": [
     lin("r_sys", "mul", k("dex_rsys", 1.0), "EP"),
     lin("s0", "add", -k("dex_s0", 0.15), "E", [0, 1]),
     lin("vagal_add", "add", k("dex_vagal", 0.3))]},
  {"classes": ["alpha2_agonist"], "ke0_per_min": 0.2, "im": {"f": 1.0, "ka_per_h": 5.96},
   "sedation": sat(0.5, 0.002, 1.5), "mac_reduction": sat(0.8, 0.0030, 1.27), "antinociception": sat(0.4, 0.002, 1.0),
   "evidence": "C", "source": "Clairance et Vss : Dent 2019 (résumé). Vc/Vp : partage placeholder D (Vc + Vp = Vss). IM : ka 5,96 h⁻¹ calculé sur le Tmax de 20,5 min (Aarnes 2023, résumé) avec t½ 36 min ; F = 1 placeholder D. ke0 placeholder D (pic de sédation 12 min après IV, Alves 2024). Épargne de MAC (recalée le 20/11/2026) : EC50 3,0 ng/mL et pente 1,27 (max 0,8 imposé, D), ajustées sur les protocoles de Pascoe 2006 (charge puis CRI de 0,5 et 3 µg/kg/h : −18 % et −59 %, cité par Acevedo-Arcique 2014, lu) aux concentrations que produit la PK du modèle à ces débits (1,1 et 6,7 ng/mL à 60 min). Les concentrations mesurées par Pascoe (0,198 et 1,903 ng/mL, résumé) sont 3 à 6 fois plus basses que celles du modèle : la PK (Dent 2019) n'est pas cohérente avec elles, écart non résolu (docs/VALIDATION_HORS_CALIBRAGE.tex). Hors échantillon : 2 µg/kg + 2 µg/kg/h, −43,4 % (Acevedo-Arcique 2014, lu). Sédation : EC50 de l'ordre des concentrations au pic de sédation (Kuusela 2000 : 5,5 ng/mL à 10 µg/kg) ; plafond 0,5 placeholder D (dose de propofol 5,2 → 2,9 mg/kg après 5 µg/kg IM, Canfrán 2016). Antinociception : placeholder D."},
  ["Dent BT et al. 2019, Am J Vet Res, doi:10.2460/ajvr.80.10.969 (résumé)", "Aarnes TK et al. 2023, Am J Vet Res, doi:10.2460/ajvr.22.10.0184 (résumé)",
   "Congdon JM et al. 2011, JAVMA, doi:10.2460/javma.239.1.81 (résumé)", "Pypendop BH, Verstegen JP 1998, Vet Surg (résumé)",
   "Pascoe PJ et al. 2006, Vet Anaesth Analg 33:97, doi:10.1111/j.1467-2995.2005.00236.x (résumé)", "Pascoe PJ 2015, Vet Anaesth Analg, doi:10.1111/vaa.12220 (résumé)",
   "Kuusela E et al. 2000, J Vet Pharmacol Ther, doi:10.1046/j.1365-2885.2000.00245.x (résumé)", "De Sant'Ana Alves R et al. 2024, Res Vet Sci, doi:10.1016/j.rvsc.2024.105229 (résumé)",
   "Canfrán S et al. 2016, Vet J, doi:10.1016/j.tvjl.2016.01.015 (résumé)"],
  "Effet biphasique : vasoconstriction périphérique (α2B) portée par la concentration PLASMATIQUE (EP), sympatholyse centrale et tonus vagal par le site effet (E). Coefficients calés par petite grille (examples/anesthesia_timing.rs) sur 10 µg/kg IM à 15 min : modèle FC −58 %, DC −57 %, PAM +32 % pour des cibles FC −55 % et DC −59 % (Congdon 2011) et PAM +36 % (Alves 2024, 2 µg/kg IV). Un bloc AV du 2e degré peut émerger du tonus vagal (av_vagal du moteur) à plus forte dose, sans terme ad hoc ; il n'apparaît pas dans ce scénario de calage, alors que Karna 2022 le décrit chez tous les chiens (avec morphine)."))
M.append(mod("acepromazine", "Acépromazine", "D",
  pk(1.0, 5.0, 10.0, 20.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 0.01, "hill": 1.0, "effects": [
     lin("ven_cap", "mul", k("ace_vencap", 0.3)), lin("hb", "mul", -0.21)]},
  {"classes": ["phenothiazine"], "ke0_per_min": 0.05, "im": {"f": 1.0, "ka_per_h": 2.0},
   "sedation": sat(0.3, 0.01, 1.0), "mac_reduction": sat(0.4, 0.005, 1.0),
   "evidence": "D", "source": "PK entièrement placeholder D (seule la t½ IV de 7,1 h est publiée, Hashem 1992). Épargne de MAC plafonnée à 40 % (halothane, Heard 1986, résumé : maximum dès 0,04 mg/kg) ; EC50 placeholder. Sédation placeholder."},
  ["Hashem A et al. 1992, Dtsch Tierarztl Wochenschr, PMID 1425314 (résumé)", "Rangel JPP et al. 2020, Vet Anaesth Analg, doi:10.1016/j.vaa.2020.02.007 (résumé)",
   "Sutil DV et al. 2017, Vet Anaesth Analg, doi:10.1016/j.vaa.2016.11.012 (résumé)", "Heard DJ et al. 1986, Am J Vet Res, doi:10.2460/ajvr.1986.47.10.2113 (résumé)"],
  "Blocage α1 : veinodilatation (ven_cap × (1 + 2·E), borne du balayage) pour une PAM −12 % chez le vigile (cible −17 % sans changement de RVS, Rangel 2020 : écart signalé) ; hémoglobine −21 % à effet maximal (CaO2 −21 %, Rangel 2020 ; séquestration splénique, Sutil 2017). Pas d'analgésie."))
M.append(mod("methadone", "Méthadone", "C",
  pk(3.0, 4.0, 25.0, 30.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 0.015, "hill": 1.3, "effects": OPIOID_EFFECTS},
  {"classes": ["opioid"], "ke0_per_min": 0.1, "im": {"f": 0.79, "ka_per_h": 2.16}, "sc": {"f": 0.79, "ka_per_h": 2.16},
   "sedation": sat(0.4, 0.015, 1.3), "mac_reduction": sat(k("meth_macmax", 0.6), k("meth_macec50", 0.02), 1.3), "antinociception": sat(0.8, 0.017, 1.3),
   "evidence": "C", "source": "SC : F 79 % et ka 2,16 h⁻¹ calculé sur le Tmax de 1,26 h (Ingvast-Larsson 2010, résumé) ; IM non publiée : valeurs SC reprises (D). Épargne de MAC calée sur Credie 2010 (1 mg/kg : −48 % à 2,5 h et −30 % à 5 h ; 0,5 mg/kg : −35 % et −15 %, résumé, voie supposée IV). Antinociception : EC50 17 ng/mL (antinociception au-delà de 17 ng/mL, Amon 2021) ; plafond D. Sédation et ke0 : placeholders D."},
  ["Ingvast-Larsson C et al. 2010, Vet Anaesth Analg, doi:10.1111/j.1467-2995.2009.00476.x (résumé)", "Credie RG et al. 2010, Vet Anaesth Analg 37:240 (résumé)",
   "Amon T et al. 2021, BMC Vet Res (PMC7814539, lu)", "Mills T et al. 2025, J Vet Pharmacol Ther (PMC12415812, lu)"],
  "PK et PD hémodynamique reprises du module opioide 1.1.0 (méthadone, chien vigile). Sous halogéné, la dépression ventilatoire s'additionne à celle de l'agent inhalé (pas de terme de synergie)."))
fa_vc, fa_vp, fa_q = 4.05, 0.95, 116.0
M.append(mod("fentanyl", "Fentanyl", "C",
  pk(fa_vc, fa_vp, 77.9, fa_q),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 0.00365, "hill": 1.3, "effects": OPIOID_EFFECTS},
  {"classes": ["opioid"], "ke0_per_min": 0.15,
   "sedation": sat(0.4, 0.00264, 1.3), "mac_reduction": sat(0.84, 0.00264, 1.0), "antinociception": sat(0.9, 0.00264, 1.3),
   "evidence": "C", "source": "PK : Sano 2006 (résumé) : Cl 77,9 mL/min/kg, Vd 5 L/kg, t½α 4,5 min, t½β 45,7 min ; Vc 4,05, Vp 0,95 L/kg et Q 116 mL/min/kg calculés (seule solution bicompartimentale compatible avec ces quatre valeurs). Épargne de MAC : plafond 84 % et EC50 2,64 ng/mL (Machado 2022, résumé) ; pente 1 placeholder D. EC50 hémodynamique 3,65 ng/mL (FC, Machado 2022). Antinociception et sédation : EC50 reprises de la MAC, plafonds D ; ke0 D."},
  ["Sano T et al. 2006, Vet Anaesth Analg, doi:10.1111/j.1467-2995.2005.00266.x (résumé)", "Machado ML et al. 2022, Vet Anaesth Analg, doi:10.1016/j.vaa.2021.09.001 (résumé)",
   "Ueyama Y et al. 2009, Am J Vet Res 70:1459, doi:10.2460/ajvr.70.12.1459 (résumé)"],
  "Effets hémodynamiques et ventilatoires : forme et amplitudes du module opioide (classe, D pour le fentanyl), EC50 propre."))
M.append(mod("propofol", "Propofol", "C",
  pk(0.8, 4.1, 58.6, 60.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 3.0, "hill": 2.0, "effects": [
     lin("r_sys", "mul", -k("prop_rsys", 0.4), "EP"), apnea(k("prop_apnea", 1.0)), lin("baro_gain", "mul", -k("prop_baro", 0.0), "EP")]},
  {"classes": ["hypnotic"], "ke0_per_min": 0.696, "c_imm_mg_L": 6.2,
   "evidence": "C", "source": "Cl 58,6 mL/kg/min et Vss 4,9 L/kg (Nolan 1993, résumé) ; partage Vc 0,8 / Vp 4,1 L/kg et Q placeholders D (Vc non publié). ke0 0,696 min⁻¹ (Lee 2009, résumé). c_imm 6,2 mg/L : ajusté sur la réduction de MAC du sévoflurane (1,3 / 2,5 / 4,4 µg/mL → −20,5 / −43 / −68 %, Singsank-Coats 2015, résumé ; contribution Ce/c_imm ≈ 0,16 MAC par µg/mL) ; il place la perte de conscience (indice 0,4) vers 2,5 µg/mL, cohérent avec Musk 2005 (65 % d'inductions à 2,5 µg/mL, 100 % à 3,5) et la cible de 3 µg/mL de Beths 2001."},
  ["Nolan AM et al. 1993, J Vet Pharmacol Ther, doi:10.1111/j.1365-2885.1993.tb00180.x (résumé)", "Lee SH et al. 2009, Br J Pharmacol (PMC2807660, résumé)",
   "Singsank-Coats J et al. 2015, Can J Vet Res (PMC4365712, résumé)", "Musk GC et al. 2005, Vet Rec, doi:10.1136/vr.157.24.766 (résumé)",
   "Cattai A et al. 2018, BMC Vet Res (PMC6134702, lu)", "Bigby SE et al. 2017, Vet Anaesth Analg (résumé)", "Maney JK et al. 2013, Vet Anaesth Analg, doi:10.1111/vaa.12006 (résumé)"],
  "Vasodilatation (r_sys) et émoussement du baroréflexe (baro_gain, mécanisme retenu, forme D) calés ensemble pour PAM −31 % au nadir et FC +62 % (88 → 61 mmHg, 65 → 105/min, Cattai 2018, lu, après acépromazine + méthadone) ; la tachycardie vient du baroréflexe du moteur. Effets hémodynamiques portés par la concentration PLASMATIQUE (EP) : nadir de PAM à 55 s, avant l'équilibre du site effet (intubation à 140 s, Cattai 2018). Apnée d'induction : commande ventilatoire max(0, 1 − 1,23·EP), portée par le pic PLASMATIQUE : une injection rapide l'abolit, et le CO2 ne la relance qu'une fois la concentration redescendue. Calée sur la dépendance à la vitesse (apnée chez 100 % des chiens à 4 mg/kg/min contre 25 % à 1 mg/kg/min, Bigby 2017 ; 59 à 64 % à 4 mg/kg, Murison 2001) : 5,2 mg/kg en 30 s sans prémédication donnent 56 s d'apnée, 1,8 mg/kg à 1 mg/kg/min après prémédication aucune. La vitesse d'injection se règle par la durée du bolus (30 s par défaut, iv_push_s du modèle)."))
M.append(mod("alfaxalone", "Alfaxalone", "C",
  pk(0.4, 0.54, 26.0, 8.5),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 1.0, "hill": 2.0, "effects": [
     lin("r_sys", "mul", -k("alf_rsys", 0.15), "EP"), apnea(k("alf_apnea", 1.0))]},
  {"classes": ["hypnotic"], "ke0_per_min": 0.7, "c_imm_mg_L": k("alf_cimm", 2.0),
   "evidence": "C", "source": "PK : Vss 0,94 L/kg, Cl 26 mL/kg/min, Q 8,5 mL/kg/min (Dehuisser 2019, résumé) ; partage Vc/Vp placeholder D. ke0 placeholder D (classe). c_imm calé pour que la dose d'induction publiée sans prémédication (2,6–2,8 mg/kg, Maney 2013, Hampton 2019) atteigne la perte de conscience avec la même marge que le propofol à 5,2 mg/kg (calage par session)."},
  ["Dehuisser V et al. 2019, Vet Anaesth Analg, doi:10.1016/j.vaa.2019.04.014 (résumé)", "Ferré PJ et al. 2006, Vet Anaesth Analg, doi:10.1111/j.1467-2995.2005.00264.x (résumé)",
   "Hampton CE et al. 2019, Am J Vet Res, doi:10.2460/ajvr.80.1.33 (résumé)", "Sarotti D et al. 2024, Front Vet Sci (PMC11422344, lu)"],
  "Hémodynamique : PAM peu modifiée (Sarotti 2024, lu ; hypotension 1/14), FC +54 % (Hampton 2019) : vasodilatation modérée placeholder D. Apnée : commande ventilatoire max(0, 1 − 0,92·EP) portée par le pic plasmatique, calée sur la dépendance à la vitesse d'injection (100 % à 2 mg/kg/min contre 25 % à 0,5 mg/kg/min, Bigby 2017) : hypopnée profonde en injection rapide après prémédication, ventilation conservée en injection lente."))
M.append(mod("ketamine", "Kétamine", "C",
  pk(0.371, 3.69, 58.2, 60.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 2.0, "hill": 1.5, "effects": [
     lin("s0", "add", k("ket_s0", 0.3), "E", [0, 1]), lin("vent_drive", "mul", -0.2)]},
  {"classes": ["dissociative"], "ke0_per_min": 0.5, "c_imm_mg_L": k("ket_cimm", 20.0),
   "mac_reduction": sat(0.922, 2.945, 3.01), "antinociception": sat(0.4, 1.0, 1.0), "antihyperalgesia": sat(0.8, 0.2, 1.0),
   "evidence": "C", "source": "PK : Vc 0,371 L/kg, Vss 4,06 L/kg, Cl 58,2 mL/min/kg (Pypendop & Ilkiw 2005, résumé) ; Q placeholder D. Épargne de MAC de l'isoflurane : Emax 92,2 %, EC50 2,945 µg/mL, γ 3,01 (Pypendop 2007, résumé, sur les données de Solano 2006). c_imm (perte de conscience) calé sur la dose d'induction de 6,1 mg/kg avec diazépam (Hampton 2019) : D. Antinociception, effet anti-hyperalgésique (NMDA) à faible concentration et ke0 : placeholders D."},
  ["Pypendop BH, Ilkiw JE 2005, Am J Vet Res, doi:10.2460/ajvr.2005.66.2034 (résumé)", "Pypendop BH et al. 2007, Vet Anaesth Analg, doi:10.1111/j.1467-2995.2006.00324.x (résumé)",
   "Solano AM et al. 2006, Am J Vet Res, doi:10.2460/ajvr.67.1.21 (résumé)", "Hampton CE et al. 2019, Am J Vet Res (résumé)", "Muir WW et al. 2003, Am J Vet Res 64:1155 (résumé)"],
  "Anesthésie dissociative : réflexe palpébral conservé, globe central. Stimulation sympathique (s0 + 0,8·E, plafond du balayage) : FC +23 % après 6,1 mg/kg + midazolam pour une cible de +75 % (Hampton 2019, kétamine-diazépam) ; le baroréflexe du moteur compense : écart signalé. Remarque : la contribution hypnotique (c_imm) et l'épargne de MAC se cumulent dans la profondeur Δ : simplification signalée."))
M.append(mod("midazolam", "Midazolam", "C",
  pk(1.0, 2.0, 27.0, 30.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 0.5, "hill": 1.0, "effects": [lin("vent_drive", "mul", -0.15)]},
  {"classes": ["benzodiazepine"], "ke0_per_min": 0.3, "im": {"f": 0.9, "ka_per_h": 4.0},
   "sedation": sat(0.5, 0.3, 1.0), "mac_reduction": sat(0.6, 0.4, 1.0),
   "evidence": "C", "source": "PK : Vd 3,0 L/kg, Cl 27 mL/kg/min (Court & Greenblatt 1992, résumé) ; partage Vc/Vp, Q, IM et ke0 placeholders D. Sédation plafonnée à 0,5 : la cible de propofol à l'intubation passe de 3,0 à 1,5 µg/mL avec 0,2 mg/kg (Minghella 2016, résumé). Épargne de MAC : enflurane −55 % à 949 ng/mL (Hall 1988, résumé) ; plafond 0,6 et EC50 D."},
  ["Court MH, Greenblatt DJ 1992, J Vet Pharmacol Ther, doi:10.1111/j.1365-2885.1992.tb01026.x (résumé)", "Hall RI et al. 1988, J Pharmacokinet Biopharm, doi:10.1007/bf01062136 (résumé)",
   "Minghella E et al. 2016, Vet Anaesth Analg, doi:10.1111/vaa.12336 (résumé)", "Aguilera R et al. 2020, Vet Anaesth Analg (résumé)"],
  "Peu d'effets hémodynamiques ; légère dépression ventilatoire placeholder D. L'excitation paradoxale du chien vigile n'est pas modélisée."))
M.append(mod("lidocaine", "Lidocaïne", "C",
  pk(0.4, 0.98, 27.5, 40.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 3.0, "hill": 1.0, "effects": []},
  {"classes": ["local_anesthetic"], "ke0_per_min": 0.5, "im": {"f": 0.919, "ka_per_h": 7.74},
   "mac_reduction": sat(0.7, 3.15, 1.36), "antinociception": sat(0.3, 3.0, 1.0),
   "local": {"onset_min": 5.0, "duration_min": 60.0, "efficacy": 0.9},
   "evidence": "C", "source": "PK : Cl 27,5 mL/min/kg, Vss 1,38 L/kg (Ngo 1997, résumé) ; partage Vc/Vp et Q placeholders D. IM : ka 7,74 h⁻¹, fraction absorbée 91,9 % (Wilcke 1983, résumé). Épargne de MAC : Emax ajusté sur Valverde 2004 (1,5 µg/mL −18,7 % ; 4,5 µg/mL −43,3 %, résumé) avec plafond 0,7 imposé (D) : EC50 3,15 mg/L, pente 1,36. Bloc locorégional (délai, durée, efficacité) et antinociception systémique : placeholders D."},
  ["Ngo LY et al. 1997, J Pharm Sci, doi:10.1021/js960399i (résumé)", "Wilcke JR et al. 1983, J Vet Pharmacol Ther, doi:10.1111/j.1365-2885.1983.tb00454.x (résumé)",
   "Valverde A et al. 2004, Vet Anaesth Analg 31:264, doi:10.1111/j.1467-2995.2004.00165.x (résumé)", "Wilcke JR et al. 1983, J Vet Pharmacol Ther, doi:10.1111/j.1365-2885.1983.tb00386.x (résumé)"],
  "Toxicité non modélisée (le moteur n'a pas d'état convulsif) : extension tonique à 8,21 µg/mL chez le chien (Wilcke 1983) ; ne pas dépasser cette concentration dans les scénarios."))
M.append(mod("bupivacaine", "Bupivacaïne (bloc locorégional)", "D",
  pk(1.0, 2.0, 10.0, 20.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 1.0, "hill": 1.0, "effects": []},
  {"classes": ["local_anesthetic"], "local": {"onset_min": 15.0, "duration_min": 240.0, "efficacy": 0.95},
   "evidence": "D", "source": "Module de bloc uniquement : délai, durée et efficacité placeholders D. PK systémique placeholder sans effet."},
  [], "À n'utiliser qu'en bloc locorégional (événement local_block)."))
M.append(mod("atipamezole", "Atipamézole", "C",
  pk(0.5, 1.0, 9.23, 20.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 1.0, "hill": 1.0, "effects": []},
  {"classes": ["antagonist"], "ke0_per_min": 0.5, "im": {"f": 1.0, "ka_per_h": 9.56},
   "antagonist": {"targets": ["dexmedetomidine"], "ki_mg_L": 0.005},
   "evidence": "C", "source": "PK IM : Cl/F 554 mL/h/kg (9,23 mL/min/kg en supposant F = 1, D), Vz/F 1,49 L/kg (partage Vc/Vp D), ka 9,56 h⁻¹ calculé sur le Tmax de 0,35 h et la t½ de 1,8 h (Cowan 2026, lu). Ki et ke0 placeholders D (éveil en 3 à 7 min après atipamézole IM, Vainio & Vähä-Vahe 1990, résumé)."},
  ["Cowan R et al. 2026, J Vet Pharmacol Ther (PMC13159760, lu)", "Vainio O, Vähä-Vahe T 1990, J Vet Pharmacol Ther, doi:10.1111/j.1365-2885.1990.tb00742.x (résumé)",
   "Turunen H et al. 2019, Am J Vet Res, doi:10.2460/ajvr.80.10.912 (résumé)"],
  "Antagonisme compétitif de la dexmédétomidine (EC50 de toutes ses courbes multipliées par 1 + C/Ki). Rechute de sédation possible quand l'atipamézole s'élimine avant l'agoniste (Turunen 2019)."))
M.append(mod("naloxone", "Naloxone", "C",
  pk(2.1, 1.0, 58.0, 30.0),
  {"type": "emax", "emax": 1.0, "ec50_mg_L": 1.0, "hill": 1.0, "effects": []},
  {"classes": ["antagonist"], "ke0_per_min": 0.5, "im": {"f": 1.0, "ka_per_h": 6.0},
   "antagonist": {"targets": ["methadone", "fentanyl"], "ki_mg_L": 0.002},
   "evidence": "C", "source": "t½ IV 37 min et Cmax 18,8 ng/mL après 0,04 mg/kg (Wahler 2019, résumé) : Vc ≈ 2,1 L/kg et Cl ≈ 58 mL/min/kg calculés (approximation, C/D) ; Vp, IM, Ki et ke0 placeholders D (antagonisation de la méthadone en < 5 min après 0,1 mg/kg IM, Mills 2025, lu)."},
  ["Wahler BM et al. 2019, Am J Vet Res, doi:10.2460/ajvr.80.7.696 (résumé)", "Mills T et al. 2025, J Vet Pharmacol Ther (PMC12415812, lu)"],
  "Renarcotisation possible si la naloxone s'élimine avant l'opioïde (t½ 37 min contre 3,9 h pour la méthadone)."))
os.makedirs(OUT, exist_ok=True)
for m in M:
    json.dump(m, open(f"{OUT}/{m['id']}.json", "w"), ensure_ascii=False, indent=1)
    open(f"{OUT}/{m['id']}.json", "a").write("\n")
print(len(M), "modules")
