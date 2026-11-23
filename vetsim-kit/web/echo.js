// Échocardiographie virtuelle (prototype) : image formée par une physique simplifiée sur une
// scène 3D dont la géométrie vient du modèle (API `echo`).
// - Géométrie : diamètre, longueur et paroi du VG (volume myocardique conservé), croissant du
//   VD, oreillettes observées (volumes), ouvertures valvulaires suivant les débits : tout est
//   calculé par le moteur à chaque instant ; ce fichier ne fait que placer ces formes dans un
//   thorax schématique (`render` de modules/echo.json, evidence D).
// - Image : diffuseurs attachés aux tissus (le speckle suit le myocarde), réflexion spéculaire
//   aux interfaces (impédances), atténuation en f·profondeur, ombre des côtes, réverbération
//   pleurale (lignes A), réponse impulsionnelle axiale et latérale (focalisation), compression
//   logarithmique, gain, TGC.
// - Sonde : point de contact sur la paroi thoracique et trois angles ; option gyroscope du
//   téléphone (DeviceOrientation).
// Usage pédagogique uniquement.

const D2R = Math.PI / 180;
const T = { AIR: 0, WALL: 1, BONE: 2, LUNG: 3, FAT: 4, PERI: 5, MYO: 6, BLOOD: 7, VALVE: 8 };
const TISSUE_KEY = ['', 'wall', 'bone', 'lung', 'fat', 'peri', 'myo', 'blood', 'valve'];

// ------------------------------------------------------------------ vecteurs
const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a, k) => [a[0] * k, a[1] * k, a[2] * k];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = (a) => { const l = Math.hypot(a[0], a[1], a[2]) || 1; return [a[0] / l, a[1] / l, a[2] / l]; };
const rotAbout = (v, k, ang) => { // Rodrigues
  const c = Math.cos(ang), s = Math.sin(ang), kv = cross(k, v), kd = dot(k, v);
  return [v[0] * c + kv[0] * s + k[0] * kd * (1 - c), v[1] * c + kv[1] * s + k[1] * kd * (1 - c), v[2] * c + kv[2] * s + k[2] * kd * (1 - c)];
};

function bulletR(d, l, z) {
  if (z < 0 || z > l) return 0;
  const h = l / 2;
  if (z <= h) return d / 2;
  const u = (z - h) / h;
  return (d / 2) * Math.sqrt(Math.max(0, 1 - u * u));
}

// ------------------------------------------------------------------ bruit attaché aux tissus
function hash3(x, y, z) {
  let h = (x * 374761393 + y * 668265263 + z * 2147483647) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return h >>> 0;
}
const SPK = new Float32Array(2);
const CELL = 0.03; // cm : maille des diffuseurs (plus fine que la réponse impulsionnelle)
function scatter(mx, my, mz) {
  const h1 = hash3(Math.floor(mx / CELL), Math.floor(my / CELL), Math.floor(mz / CELL));
  const h2 = Math.imul(h1 ^ 0x9e3779b9, 0x85ebca6b) >>> 0;
  const u1 = (h1 + 1) / 4294967297, u2 = h2 / 4294967296;
  const r = Math.sqrt(-2 * Math.log(u1));
  SPK[0] = r * Math.cos(2 * Math.PI * u2); SPK[1] = r * Math.sin(2 * Math.PI * u2);
}

// ------------------------------------------------------------------ scène statique
function buildScene(r) {
  const E = r.echo, R = E.render, bw = E.bw, k13 = Math.cbrt(bw), st = E.static;
  const th = R.thorax, X = th.width_cm_per_kg13 * k13 / 2, Y = th.depth_cm_per_kg13 * k13 / 2;
  const wall = th.wall_cm_per_kg13 * k13;
  const ribSp = th.rib_spacing_cm_per_kg13 * k13, ribW = th.rib_width_cm_per_kg13 * k13;
  const hz = R.heart;
  // axe du VG (base → apex) : ventral, caudal, vers la gauche
  const tilt = hz.long_axis_tilt_deg * D2R, yaw = hz.long_axis_yaw_deg * D2R;
  const e3 = norm([Math.sin(yaw), -Math.cos(tilt), -Math.sin(tilt)]);
  // septum (VG → VD) : vers la droite et crânialement, tourné autour de l'axe
  let e1 = norm(sub([-1, 0, 0.35], mul(e3, dot([-1, 0, 0.35], e3))));
  e1 = norm(rotAbout(e1, e3, hz.septum_roll_deg * D2R));
  const e2 = cross(e3, e1);
  const lref = st.lv_ref;
  const heartLen = lref.l + lref.w + 0.6 * Math.cbrt(E.truth.la_max || 20) * 1.3;
  const center = [hz.center_frac[0] * X, hz.center_frac[1] * Y, hz.center_frac[2] * Y];
  // origine du repère cardiaque : centre de la base du VG
  const origin = sub(center, mul(e3, heartLen / 2 - 0.6 * Math.cbrt(E.truth.la_max || 20) * 1.3));
  const nR = th.n_ribs, ribs = [];
  for (let i = 0; i < nR; i++) ribs.push(center[2] + (i - (nR - 1) / 2) * ribSp);
  const tis = TISSUE_KEY.map((k) => (k ? R.tissues[k] : { alpha_db_cm_mhz: 0, backscatter: 0, z: 0.0004 }));
  return {
    bw, X, Y, wall, Xi: X - wall, Yi: Y - wall, Xr: X - 0.25 * wall, Yr: Y - 0.25 * wall,
    ribs, ribW, notchY: hz.lung_notch_y_frac * Y, e1, e2, e3, origin, center,
    zLo: center[2] - heartLen * 0.75, zHi: center[2] + heartLen * 0.75, tis,
    geo: E.geometry, st,
  };
}

// Géométrie instantanée (interpolée sur les séries du moteur).
function frameGeometry(r, sc, t) {
  const S = r.echo.series, fs = r.fs, n = S.lv_d.length;
  const x = Math.min(Math.max(t * fs, 0), n - 1.001), i = Math.floor(x), f = x - i;
  const at = (a) => a[i] + (a[i + 1] - a[i]) * f;
  const d = at(S.lv_d), l = at(S.lv_l), w = at(S.lv_w), rvt = at(S.rv_t);
  const la = [at(S.la_ax[0]), at(S.la_ax[1]), at(S.la_ax[2])], ra = [at(S.ra_ax[0]), at(S.ra_ax[1]), at(S.ra_ax[2])];
  const st = sc.st, g = sc.geo;
  const epiD = d + 2 * w, epiL = l + w;
  const lr = g.rv_length_fraction * epiL;
  const G = {
    d, l, w, epiD, epiL, rvt, lr, rvw: st.rv_wall, cosCov: Math.cos(g.rv_coverage_deg * D2R),
    // anneau mitral (repère cardiaque, plan de la base)
    M: [-0.18 * d, -0.12 * d], rm: 0.5 * st.mitral_annulus_d * Math.pow(d / st.lv_ref.d, 0.3), lfm: st.leaflet_l,
    am: Math.pow(at(S.mi_open), 0.7) * 75 * D2R,
    // racine aortique
    A: [0.42 * d, 0.28 * d, -0.15], ao: norm([0.1, 0.25, -1]), rao: st.ao_d / 2, aoLen: 3.2 * st.ao_d,
    aa: Math.pow(at(S.ao_open), 0.7) * 80 * D2R,
    // tricuspide (base du croissant du VD)
    Tc: [epiD / 2 + rvt / 2, -0.05 * d], rt: Math.max(0.3, 0.55 * rvt + 0.3), at: Math.pow(at(S.tr_open), 0.7) * 70 * D2R,
    la, ra,
  };
  G.LAc = [G.M[0] - 0.12 * la[0], G.M[1] - 0.22 * la[1], -(la[2] / 2 - 0.25)];
  G.RAc = [epiD / 2 + rvt * 0.6 + 0.15 * ra[0], -0.25 * ra[1], -(ra[2] / 2 - 0.25)];
  // sac péricardique : ellipsoïde englobante
  const top = Math.min(-la[2] + 0.25, -ra[2] + 0.25, -G.aoLen * 0.6);
  const bot = epiL + 0.25;
  G.sac = { c: [0.25 * (rvt + G.rvw), 0, (top + bot) / 2], a: [epiD / 2 + rvt + G.rvw + 0.55, Math.max(epiD / 2, la[1] * 0.75) + 0.6, (bot - top) / 2 + 0.25] };
  G.laW = 0.18; G.raW = 0.15; G.aoW = 0.16; G.vth = 0.07;
  return G;
}

// ------------------------------------------------------------------ classification d'un point
const MAT = new Float64Array(3);
function classify(sc, G, x, y, z, h1, h2, h3, t) {
  const qo = (x / sc.X) ** 2 + (y / sc.Y) ** 2;
  MAT[0] = x; MAT[1] = y; MAT[2] = z;
  if (qo > 1) return T.AIR;
  const qi = (x / sc.Xi) ** 2 + (y / sc.Yi) ** 2;
  if (qi > 1) {
    const qr = (x / sc.Xr) ** 2 + (y / sc.Yr) ** 2;
    if (qr <= 1) {
      for (const rz of sc.ribs) if (Math.abs(z - rz) < sc.ribW / 2) return T.BONE;
    }
    return T.WALL;
  }
  const sac = G.sac;
  const sv = ((h1 - sac.c[0]) / sac.a[0]) ** 2 + ((h2 - sac.c[1]) / sac.a[1]) ** 2 + ((h3 - sac.c[2]) / sac.a[2]) ** 2;
  if (sv >= 1) {
    if (y > sc.notchY || z < sc.zLo || z > sc.zHi) return T.LUNG;
    return T.FAT;
  }
  const id = heartTissue(G, h1, h2, h3, t);
  if (id) return id;
  return sv > 0.93 ? T.PERI : T.FAT;
}

// Feuillet valvulaire en entonnoir : charnière au rayon `ra`, ouvert de l'angle `al` vers +s.
function leaflet(rho, s, ra, lf, al, thk) {
  if (rho > ra * 1.02) return false;
  const ca = Math.cos(al);
  if (rho < ra - lf * ca) return false;
  const sl = (ra - rho) * Math.tan(al);
  return Math.abs(s - sl) * ca < thk / 2;
}

function heartTissue(G, h1, h2, h3, t) {
  // racine aortique et valve aortique
  const r1 = h1 - G.A[0], r2 = h2 - G.A[1], r3 = h3 - G.A[2];
  const sa = r1 * G.ao[0] + r2 * G.ao[1] + r3 * G.ao[2];
  const p1 = r1 - sa * G.ao[0], p2 = r2 - sa * G.ao[1], p3 = r3 - sa * G.ao[2];
  const rhoA = Math.sqrt(p1 * p1 + p2 * p2 + p3 * p3);
  if (sa > -0.6 && sa < G.aoLen && rhoA < G.rao + G.aoW) {
    if (sa > -0.25 && sa < G.rao * 1.2 && leaflet(rhoA, sa, G.rao, G.rao * 1.02, G.aa, G.vth)) { matHeart(h1, h2, h3, 0); return T.VALVE; }
    if (rhoA < G.rao) { matBlood(h1, h2, h3, t); return T.BLOOD; }
    if (sa > 0.05) { matHeart(h1, h2, h3, 0); return T.PERI; }
  }
  // valve mitrale (s'ouvre vers l'apex, +h3) et vestibule
  const m1 = h1 - G.M[0], m2 = h2 - G.M[1], rhoM = Math.sqrt(m1 * m1 + m2 * m2);
  if (rhoM < G.rm * 1.05 && h3 > -0.5 && h3 < G.lfm + 0.1) {
    if (leaflet(rhoM, h3, G.rm, G.lfm, G.am, G.vth)) { matHeart(h1, h2, h3, 1); return T.VALVE; }
    if (h3 <= 0 && rhoM < G.rm) { matBlood(h1, h2, h3, t); return T.BLOOD; }
  }
  // valve tricuspide
  const t1 = h1 - G.Tc[0], t2 = h2 - G.Tc[1], rhoT = Math.sqrt(t1 * t1 + t2 * t2);
  if (rhoT < G.rt * 1.05 && h3 > -0.5 && h3 < G.rt + 0.1) {
    if (leaflet(rhoT, h3, G.rt, G.rt * 0.9, G.at, G.vth)) { matHeart(h1, h2, h3, 2); return T.VALVE; }
    if (h3 <= 0 && rhoT < G.rt) { matBlood(h1, h2, h3, t); return T.BLOOD; }
  }
  const rho = Math.sqrt(h1 * h1 + h2 * h2);
  // VG : cavité et myocarde (coordonnées matérielles normalisées : le speckle suit la paroi)
  if (h3 >= 0 && h3 <= G.epiL) {
    const rin = bulletR(G.d, G.l, h3), rout = bulletR(G.epiD, G.epiL, h3);
    if (rho < rin) { matBlood(h1, h2, h3, t); return T.BLOOD; }
    if (rho < rout) {
      const nu = (rho - rin) / Math.max(1e-6, rout - rin), ph = Math.atan2(h2, h1);
      const rr = 2 + nu * 0.9, zz = (h3 / G.epiL) * 8;
      MAT[0] = 1000 + rr * Math.cos(ph) * 1.6; MAT[1] = rr * Math.sin(ph) * 1.6; MAT[2] = zz;
      return T.MYO;
    }
  }
  // VD : croissant et paroi libre
  if (h3 >= 0 && h3 <= G.lr + G.rvt + G.rvw && rho > 1e-6 && h1 / rho >= G.cosCov) {
    const ro = bulletR(G.epiD + 2 * G.rvt, G.lr + G.rvt, h3), rw = bulletR(G.epiD + 2 * G.rvt + 2 * G.rvw, G.lr + G.rvt + G.rvw, h3);
    if (rho < ro) { matBlood(h1, h2, h3, t); return T.BLOOD; }
    if (rho < rw) { matHeart(h1, h2, h3, 3); return T.MYO; }
  }
  // oreillettes (ellipsoïdes, paroi mince)
  for (const [c, a, wth, k] of [[G.LAc, G.la, G.laW, 4], [G.RAc, G.ra, G.raW, 5]]) {
    const q = ((h1 - c[0]) / (a[1] / 2)) ** 2 + ((h2 - c[1]) / (a[0] / 2)) ** 2 + ((h3 - c[2]) / (a[2] / 2)) ** 2;
    if (q < 1) { matBlood(h1, h2, h3, t); return T.BLOOD; }
    const g = 1 + wth / ((a[0] + a[1] + a[2]) / 6);
    if (q < g * g) { matHeart(h1, h2, h3, k); return T.MYO; }
  }
  return 0;
}
function matHeart(h1, h2, h3, k) { MAT[0] = 2000 + 100 * k + h1; MAT[1] = h2; MAT[2] = h3; }
function matBlood(h1, h2, h3, t) { MAT[0] = 5000 + h1 + 7.3 * t; MAT[1] = h2 - 3.1 * t; MAT[2] = h3 + 5.7 * t; }

// ------------------------------------------------------------------ sonde
function probeFrame(sc, pose) {
  const side = pose.side === 'left' ? 1 : -1, psi = pose.psi * D2R;
  const P = [side * sc.X * Math.cos(psi), sc.Y * Math.sin(psi), sc.center[2] + pose.z];
  const n = norm([-P[0] / (sc.X * sc.X), -P[1] / (sc.Y * sc.Y), 0]); // normale entrante
  const tz = norm(sub([0, 0, 1], mul(n, n[2])));
  const tc = cross(n, tz);
  const b = norm(add(add(n, mul(tz, Math.tan(pose.tiltZ * D2R))), mul(tc, Math.tan(pose.tiltC * D2R))));
  const L0 = norm(sub(tz, mul(b, dot(tz, b))));
  const L = norm(add(mul(L0, Math.cos(pose.rot * D2R)), mul(cross(b, L0), Math.sin(pose.rot * D2R))));
  return { P: add(P, mul(n, 0.02)), b, L, n, tz, tc, side };
}
// Pose depuis (point de contact, faisceau, marqueur) : inverse de probeFrame.
function poseFrom(sc, P, b, L) {
  const side = P[0] >= 0 ? 'left' : 'right', sx = side === 'left' ? 1 : -1;
  const psi = Math.atan2(P[1] / sc.Y, (sx * P[0]) / sc.X) / D2R;
  const z = P[2] - sc.center[2];
  const pose = { side, psi, z, tiltZ: 0, tiltC: 0, rot: 0 };
  const f = probeFrame(sc, pose);
  pose.tiltZ = Math.atan2(dot(b, f.tz), dot(b, f.n)) / D2R;
  pose.tiltC = Math.atan2(dot(b, f.tc), dot(b, f.n)) / D2R;
  const L0 = norm(sub(f.tz, mul(b, dot(f.tz, b))));
  pose.rot = Math.atan2(dot(L, cross(b, L0)), dot(L, L0)) / D2R;
  return pose;
}
const toHeart = (sc, p) => { const q = sub(p, sc.origin); return [dot(q, sc.e1), dot(q, sc.e2), dot(q, sc.e3)]; };
const fromHeart = (sc, h) => add(sc.origin, add(add(mul(sc.e1, h[0]), mul(sc.e2, h[1])), mul(sc.e3, h[2])));
const dirFromHeart = (sc, h) => norm(add(add(mul(sc.e1, h[0]), mul(sc.e2, h[1])), mul(sc.e3, h[2])));

// Intersection du rayon p + s·u (s > 0) avec la paroi externe du thorax.
function hitWall(sc, p, u) {
  const a = (u[0] / sc.X) ** 2 + (u[1] / sc.Y) ** 2, bq = 2 * (p[0] * u[0] / sc.X ** 2 + p[1] * u[1] / sc.Y ** 2);
  const c = (p[0] / sc.X) ** 2 + (p[1] / sc.Y) ** 2 - 1;
  const s = (-bq + Math.sqrt(Math.max(0, bq * bq - 4 * a * c))) / (2 * a);
  return add(p, mul(u, s));
}

// ------------------------------------------------------------------ coupes de référence
function views(sc, G) {
  const lvMid = fromHeart(sc, [0, 0, G.l * 0.45]);
  const aoAxis = dirFromHeart(sc, G.ao);
  const laMid = fromHeart(sc, G.LAc);
  const aoAtLa = fromHeart(sc, add(G.A, mul(G.ao, Math.abs(G.LAc[2]) * 0.9)));
  return [
    { id: 'rplax4', name: 'Parasternale droite grand axe 4 cavités', side: 'right', target: lvMid, normal: sc.e2, beam: mul(sc.e1, -1), lat: mul(sc.e3, -1), lax: true },
    { id: 'rplax5', name: 'Parasternale droite grand axe, chambre de chasse', side: 'right', target: fromHeart(sc, [0.2 * G.d, 0.14 * G.d, G.l * 0.35]),
      normal: norm(cross(sc.e3, dirFromHeart(sc, [0.83, 0.55, 0]))), beam: mul(dirFromHeart(sc, [0.83, 0.55, 0]), -1), lat: mul(sc.e3, -1), lax: true },
    { id: 'rpsa', name: 'Parasternale droite petit axe (piliers)', side: 'right', target: fromHeart(sc, [0, 0, G.l * 0.55]), normal: sc.e3, beam: mul(sc.e1, -1), lat: sc.e2 },
    { id: 'rpsab', name: 'Parasternale droite petit axe, base (Ao/OG)', side: 'right', target: mul(add(aoAtLa, laMid), 0.5), normal: aoAxis, beam: mul(sc.e1, -1), lat: sc.e2 },
    { id: 'lap4', name: 'Apicale gauche 4 cavités', side: 'left', target: fromHeart(sc, [0, 0, G.l * 0.3]), normal: sc.e2, beam: mul(sc.e3, -1), lat: sc.e1, lax: true },
  ];
}

// Pose qui réalise une coupe de référence : faisceau dans le plan, fenêtre sans poumon ni côte.
function solveView(sc, G, v, t) {
  let best = null;
  for (let da = -40; da <= 40; da += 5) {
    const beam = norm(rotAbout(v.beam, v.normal, da * D2R));
    const P = hitWall(sc, v.target, mul(beam, -1));
    const b = norm(sub(v.target, P));
    const L = norm(sub(v.lat, mul(b, dot(v.lat, b))));
    // obstacle : poumon ou côte sur le rayon central et deux rayons latéraux
    let bad = 0;
    const dist = Math.hypot(...sub(v.target, P));
    for (const off of [-0.6, -0.3, 0, 0.3, 0.6]) {
      const u = norm(add(b, mul(L, off)));
      for (let s = 0.05; s < dist; s += 0.1) {
        const p = add(P, mul(u, s)), h = toHeart(sc, p);
        const id = classify(sc, G, p[0], p[1], p[2], h[0], h[1], h[2], t);
        if (id === T.LUNG || id === T.BONE) bad += 1;
      }
    }
    const sc2 = bad + Math.abs(da) * 0.02 + (((P[0] >= 0) !== (v.side === 'left')) ? 1000 : 0);
    if (!best || sc2 < best.s) best = { s: sc2, P, b, L };
  }
  // décaler le point de contact dans l'espace intercostal le plus proche
  const P = best.P;
  let rz = sc.ribs[0];
  for (const z of sc.ribs) if (Math.abs(z - P[2]) < Math.abs(rz - P[2])) rz = z;
  const dz = P[2] - rz;
  if (Math.abs(dz) < sc.ribW * 0.9) {
    const shift = (dz >= 0 ? 1 : -1) * (sc.ribW * 0.9 - Math.abs(dz) + 0.25);
    const P2 = [P[0], P[1], P[2] + shift];
    const b2 = norm(sub(v.target, P2));
    return poseFrom(sc, P2, b2, norm(sub(v.lat, mul(b2, dot(v.lat, b2)))));
  }
  return poseFrom(sc, P, best.b, best.L);
}

function viewScore(sc, f, vw) {
  const n = norm(cross(f.b, f.L));
  const ang = Math.acos(Math.min(1, Math.abs(dot(n, vw.normal)))) / D2R;
  const off = Math.abs(dot(sub(vw.target, f.P), n));
  const toT = norm(sub(vw.target, f.P));
  const dev = Math.acos(Math.min(1, dot(toT, f.b))) / D2R;
  return { ang, off, dev, q: Math.max(0, 1 - ang / 30) * Math.max(0, 1 - off / 1.5) * Math.max(0, 1 - Math.max(0, dev - 30) / 20) };
}

// ------------------------------------------------------------------ formation de l'image
function makeEngine() {
  return { NL: 0, NS: 0, re: null, im: null, env: null, tmpR: null, tmpI: null, lut: null, key: '' };
}

function setupGrid(eng, set, W, H) {
  const NL = set.lines, NS = Math.round(set.depth / 0.025);
  const key = [NL, NS, set.depth, set.sector, W, H].join(':');
  if (eng.key === key) return;
  eng.key = key; eng.NL = NL; eng.NS = NS;
  eng.re = new Float32Array(NL * NS); eng.im = new Float32Array(NL * NS);
  eng.tmpR = new Float32Array(NL * NS); eng.tmpI = new Float32Array(NL * NS);
  eng.env = new Float32Array(NL * NS);
  // table de conversion secteur → pixels
  const half = set.sector / 2 * D2R, scale = (H - 8) / set.depth, cx = W / 2;
  eng.scale = scale; eng.cx = cx;
  eng.lut = new Int32Array(W * H).fill(-1); eng.lutF = new Float32Array(W * H * 2);
  for (let py = 0; py < H; py++) {
    for (let px = 0; px < W; px++) {
      const xc = (px - cx) / scale, yc = (py - 4) / scale;
      const rr = Math.hypot(xc, yc), th = Math.atan2(xc, yc);
      if (yc <= 0 || rr >= set.depth || Math.abs(th) > half) continue;
      const li = (th + half) / (2 * half) * (NL - 1), si = rr / set.depth * (NS - 1);
      const i0 = Math.min(NL - 2, Math.floor(li)), s0 = Math.min(NS - 2, Math.floor(si));
      eng.lut[py * W + px] = i0 * NS + s0; eng.lutF[2 * (py * W + px)] = li - i0; eng.lutF[2 * (py * W + px) + 1] = si - s0;
    }
  }
}

// Calcule les lignes d'angles `thetas` (radians) : signal complexe en bande de base.
function acquire(sc, G, f, set, t, thetas, re, im, NS, seedT) {
  const dr = set.depth / NS, fMHz = set.freq, tis = sc.tis;
  const lam = 0.154 / fMHz; // cm
  const hP = toHeart(sc, f.P);
  for (let li = 0; li < thetas.length; li++) {
    const th = thetas[li], c = Math.cos(th), s = Math.sin(th);
    const u = [f.b[0] * c + f.L[0] * s, f.b[1] * c + f.L[1] * s, f.b[2] * c + f.L[2] * s];
    const hu = [dot(u, sc.e1), dot(u, sc.e2), dot(u, sc.e3)];
    let att = 0, trans = 1, prev = T.WALL, pleura = -1, pleuraA = 0;
    const base = li * NS;
    for (let k = 0; k < NS; k++) {
      const rr = (k + 0.5) * dr;
      const x = f.P[0] + u[0] * rr, y = f.P[1] + u[1] * rr, z = f.P[2] + u[2] * rr;
      const id = classify(sc, G, x, y, z, hP[0] + hu[0] * rr, hP[1] + hu[1] * rr, hP[2] + hu[2] * rr, t);
      const tp = tis[id];
      let ar = 0, ai = 0;
      if (id !== T.AIR && trans > 1e-6) {
        const amp = trans * Math.pow(10, -att / 10); // aller-retour : 2 × att/20 dB
        scatter(MAT[0], MAT[1], MAT[2]);
        const bs = Math.sqrt(tp.backscatter) * amp;
        ar = bs * SPK[0]; ai = bs * SPK[1];
        if (id !== prev) {
          const z1 = tis[prev].z, z2 = tp.z, R = ((z2 - z1) / (z2 + z1)) ** 2;
          const sp = 2.2 * Math.sqrt(R) * amp;
          ar += sp; ai += sp * 0.3;
          trans *= 1 - R;
          if (id === T.LUNG && pleura < 0) { pleura = k; pleuraA = sp; }
        }
        att += tp.alpha_db_cm_mhz * fMHz * dr;
      } else if (id === T.AIR) trans = 0;
      prev = id;
      re[base + k] = ar; im[base + k] = ai;
    }
    // réverbérations entre la sonde et la plèvre (lignes A)
    if (pleura > 0) {
      for (let m = 2; m <= 6; m++) {
        const k = pleura * m;
        if (k >= NS) break;
        const a = pleuraA * Math.pow(0.45, m - 1) * Math.pow(10, -(0.6 * fMHz * (m - 1) * pleura * dr) / 10);
        for (let j = -1; j <= 1; j++) if (k + j < NS && k + j >= 0) re[base + k + j] += a * (j === 0 ? 1 : 0.5);
      }
    }
    // bruit électronique
    for (let k = 0; k < NS; k++) {
      const h = hash3(li + seedT * 131, k, 77);
      re[base + k] += ((h & 0xffff) / 65536 - 0.5) * 2.2e-5;
      im[base + k] += ((h >>> 16) / 65536 - 0.5) * 2.2e-5;
    }
  }
  return lam;
}

// Réponse impulsionnelle : axiale (≈ 1 longueur d'onde) puis latérale (faisceau focalisé).
function psf(set, NL, NS, re, im, tr, ti, lam, dth) {
  const dr = set.depth / NS;
  const sa = Math.max(0.6, (0.6 * lam) / dr), ra = Math.ceil(2.5 * sa);
  const ka = []; for (let j = -ra; j <= ra; j++) ka.push(Math.exp(-0.5 * (j / sa) ** 2));
  for (let l = 0; l < NL; l++) {
    const b = l * NS;
    for (let k = 0; k < NS; k++) {
      let sr = 0, si = 0;
      for (let j = -ra; j <= ra; j++) {
        const q = k + j; if (q < 0 || q >= NS) continue;
        sr += re[b + q] * ka[j + ra]; si += im[b + q] * ka[j + ra];
      }
      tr[b + k] = sr; ti[b + k] = si;
    }
  }
  const A = set.aperture, F = set.focus;
  for (let k = 0; k < NS; k++) {
    const rr = (k + 0.5) * dr;
    const w0 = lam * F / A, dof = 7 * lam * (F / A) ** 2;
    const wb = Math.min(A, w0 * Math.sqrt(1 + ((rr - F) / dof) ** 2));
    const sl = Math.min(6, Math.max(0.5, (wb / 2.355) / (rr * dth)));
    const rl = Math.ceil(2.2 * sl);
    for (let l = 0; l < NL; l++) {
      let sr = 0, si = 0, ws = 0;
      for (let j = -rl; j <= rl; j++) {
        const q = l + j; if (q < 0 || q >= NL) continue;
        const wgt = Math.exp(-0.5 * (j / sl) ** 2);
        sr += tr[q * NS + k] * wgt; si += ti[q * NS + k] * wgt; ws += wgt;
      }
      re[l * NS + k] = sr / Math.sqrt(ws); im[l * NS + k] = si / Math.sqrt(ws);
    }
  }
}

function tgcAt(set, rr) {
  const u = rr / set.depth, g = set.tgc;
  const v = u < 0.5 ? g[0] + (g[1] - g[0]) * (u / 0.5) : g[1] + (g[2] - g[1]) * ((u - 0.5) / 0.5);
  return v + 2 * 0.5 * set.freq * rr; // compensation de l'atténuation moyenne des tissus mous
}

function toGray(set, re, im, env, NL, NS) {
  const dr = set.depth / NS, ref = 20 * Math.log10(Math.sqrt(0.3));
  for (let k = 0; k < NS; k++) {
    const add0 = set.gain + tgcAt(set, (k + 0.5) * dr) - ref;
    for (let l = 0; l < NL; l++) {
      const i = l * NS + k, a = Math.hypot(re[i], im[i]);
      const db = 20 * Math.log10(a + 1e-12) + add0;
      env[i] = Math.min(1, Math.max(0, (db + set.dr) / set.dr - 0.32));
    }
  }
}

// ------------------------------------------------------------------ montage
export function mountEcho(v, r, E, ui) {
  if (E.raf) cancelAnimationFrame(E.raf);
  if (E.onKey) window.removeEventListener('keydown', E.onKey);
  const sc = buildScene(r), dur = r.duration_s, tr = r.echo.truth;
  const set = E.set;
  if (!set.depth) set.depth = Math.round(Math.min(18, Math.max(5, 4.2 * Math.cbrt(r.echo.bw))) * 2) / 2;
  if (!set.focus) set.focus = set.depth * 0.5;
  if (!set.freq) set.freq = r.echo.bw < 10 ? 7 : r.echo.bw < 25 ? 5 : 3.5;
  const G0 = frameGeometry(r, sc, 0);
  const VIEWS = views(sc, G0);
  if (!E.pose) E.pose = solveView(sc, G0, VIEWS[0], 0);
  const fmt = ui.fmt;
  const slider = (id, lab, min, max, step, val, unit = '°') => `<label class="ec-sl">${lab} <b id="${id}-v">${fmt(val, step < 1 ? 1 : 0)}${unit}</b>
     <input id="${id}" type="range" min="${min}" max="${max}" step="${step}" value="${val}"></label>`;
  const P = E.pose;
  v.innerHTML = `<div class="warn">Prototype d'échocardiographie virtuelle. La géométrie des cavités vient du modèle : volumes, paroi à volume myocardique constant, OG et OD observées, ouvertures valvulaires suivant les débits. Le thorax, la disposition des structures et l'acoustique sont schématiques (evidence D) et attendent une anatomie segmentée sur scanner. Usage pédagogique uniquement.</div>
  <div class="ec">
    <div class="ec-main">
      <div class="ec-bar">
        <button class="hv-play" id="e-play" aria-label="Lecture ou pause (espace)">${E.playing ? '❚❚' : '▶'}</button>
        <span id="e-time" class="note"></span>
        <span class="hv-seg">${[0.1, 0.25, 0.5, 1].map((s) => `<button data-espeed="${s}" class="${E.speed === s ? 'on' : ''}">×${String(s).replace('.', ',')}</button>`).join('')}</span>
        <button id="e-prev" title="image précédente (←)">◀︎</button><button id="e-next" title="image suivante (→)">▶︎</button>
        <label class="inline"><input type="checkbox" id="e-mm" ${E.mmode ? 'checked' : ''}> Mode TM</label>
        <label class="inline"><input type="checkbox" id="e-flip" ${set.flip ? 'checked' : ''}> Inverser gauche-droite</label>
      </div>
      <canvas id="e-img" width="640" height="560" class="ec-img"></canvas>
      <canvas id="e-mmc" width="640" height="300" class="ec-img" ${E.mmode ? '' : 'hidden'}></canvas>
      <div class="note" id="e-perf"></div>
    </div>
    <div class="ec-side">
      ${ui.card('Sonde', `
        <div class="row"><label>Fenêtre<select id="e-side"><option value="right" ${P.side === 'right' ? 'selected' : ''}>Hémithorax droit</option><option value="left" ${P.side === 'left' ? 'selected' : ''}>Hémithorax gauche</option></select></label></div>
        ${slider('e-psi', 'Hauteur sur la paroi (ventral ↔ dorsal)', -85, 60, 0.5, P.psi)}
        ${slider('e-z', 'Position crânio-caudale', -12, 12, 0.05, P.z, ' cm')}
        ${slider('e-tz', 'Angulation crânio-caudale', -60, 60, 0.5, P.tiltZ)}
        ${slider('e-tc', 'Angulation dorso-ventrale', -60, 60, 0.5, P.tiltC)}
        ${slider('e-rot', 'Rotation (repère)', -180, 180, 1, P.rot)}
        <div class="row"><button id="e-gyro">Sonde = téléphone (gyroscope)</button><button id="e-cal" hidden>Recalibrer</button></div>
        <div class="note" id="e-gyro-st">Sur un téléphone : ouvrir cette page, tenir le téléphone comme une sonde (bord supérieur vers le thorax) puis activer. Le point de contact reste réglé par les curseurs.</div>
        <div id="e-score" class="note"></div>
        <div class="note">Aide (place la sonde sur la coupe de référence) :</div>
        <div class="ec-views">${VIEWS.map((x, i) => `<button data-view="${i}">${x.name}</button>`).join('')}</div>
        <canvas id="e-inset" width="320" height="170" class="ec-inset"></canvas>`)}
      ${ui.card('Réglages', `
        ${slider('e-freq', 'Fréquence', 2, 10, 0.5, set.freq, ' MHz')}
        ${slider('e-depth', 'Profondeur', 3, 20, 0.5, set.depth, ' cm')}
        ${slider('e-focus', 'Focale', 1, 20, 0.5, set.focus, ' cm')}
        ${slider('e-gain', 'Gain', -30, 30, 1, set.gain, ' dB')}
        ${slider('e-dr', 'Gamme dynamique', 30, 80, 1, set.dr, ' dB')}
        <div class="row">${['proche', 'milieu', 'loin'].map((k, i) => slider('e-tgc' + i, 'TGC ' + k, -20, 20, 1, set.tgc[i], ' dB')).join('')}</div>`)}
      ${ui.card('Mesures (image figée)', `
        <div class="row"><label>Mesure<select id="e-mlab">${MEAS.map((m) => `<option value="${m[0]}">${m[1]}</option>`).join('')}</select></label>
        <button id="e-mclear">Effacer</button><button id="e-truth">${E.showTruth ? 'Masquer' : 'Comparer au'} modèle</button></div>
        <div class="note">Mettre en pause, puis cliquer-glisser sur l'image (ou sur le TM) entre les deux bords à mesurer.</div>
        <div id="e-mlist"></div>`)}
    </div>
  </div>`;
  const $ = (s) => v.querySelector(s);
  const cv = $('#e-img'), ctx = cv.getContext('2d');
  const mm = $('#e-mmc'), mctx = mm.getContext('2d');
  const W = cv.width, IH = cv.height - 90; // bas : ECG
  const img = ctx.createImageData(W, IH);
  const eng = E.eng || (E.eng = makeEngine());
  const mmBuf = E.mmBuf && E.mmBuf.width === mm.width ? E.mmBuf : (E.mmBuf = mctx.createImageData(mm.width, mm.height - 50));
  E.mmT = E.mmT || [];
  let frame = 0, dirty = true;

  const geomNow = () => frameGeometry(r, sc, E.t);
  const pose = () => E.pose;

  function drawB() {
    setupGrid(eng, set, W, IH);
    const G = geomNow(), f = probeFrame(sc, pose());
    if (set.flip) f.L = mul(f.L, -1);
    const NL = eng.NL, NS = eng.NS, half = set.sector / 2 * D2R, dth = 2 * half / (NL - 1);
    const thetas = Array.from({ length: NL }, (_, i) => -half + i * dth);
    const t0 = performance.now();
    const lam = acquire(sc, G, f, set, E.t, thetas, eng.re, eng.im, NS, frame);
    psf(set, NL, NS, eng.re, eng.im, eng.tmpR, eng.tmpI, lam, dth);
    toGray(set, eng.re, eng.im, eng.env, NL, NS);
    const d = img.data, env = eng.env;
    for (let p = 0; p < W * IH; p++) {
      const i = eng.lut[p];
      let g = 0;
      if (i >= 0) {
        const fx = eng.lutF[2 * p], fy = eng.lutF[2 * p + 1];
        g = (env[i] * (1 - fy) + env[i + 1] * fy) * (1 - fx) + (env[i + NS] * (1 - fy) + env[i + NS + 1] * fy) * fx;
      }
      const c = Math.round(255 * g);
      d[4 * p] = c; d[4 * p + 1] = c; d[4 * p + 2] = Math.min(255, c + 4); d[4 * p + 3] = 255;
    }
    ctx.fillStyle = '#000'; ctx.fillRect(0, 0, W, cv.height);
    ctx.putImageData(img, 0, 0);
    overlay(f, G);
    drawEcg();
    $('#e-perf').textContent = `${NL} lignes × ${NS} échantillons, ${fmt(performance.now() - t0, 0)} ms par image`;
    frame++;
    return { f, G };
  }

  function overlay(f, G) {
    ctx.save();
    ctx.font = '12px system-ui'; ctx.fillStyle = '#9fd3c7';
    ctx.fillText(`${fmt(set.freq, 1)} MHz  ·  prof. ${fmt(set.depth, 1)} cm  ·  gain ${set.gain} dB  ·  GD ${set.dr} dB`, 8, 16);
    ctx.fillText(`FC ${fmt(r.hr, 0)}/min`, W - 90, 16);
    // échelle de profondeur
    ctx.strokeStyle = '#6aa'; ctx.fillStyle = '#6aa';
    for (let c = 1; c < set.depth; c++) {
      const y = 4 + c * eng.scale;
      ctx.fillRect(W - 14, y, c % 5 === 0 ? 10 : 5, 1);
      if (c % 5 === 0) ctx.fillText(String(c), W - 34, y + 4);
    }
    // repère de la sonde (point d'index)
    const half = set.sector / 2 * D2R, rr = 1.2;
    ctx.fillStyle = '#ffb000';
    ctx.beginPath(); ctx.arc(eng.cx + Math.sin(half) * rr * eng.scale, 4 + Math.cos(half) * rr * eng.scale, 4, 0, 7); ctx.fill();
    // ligne TM
    if (E.mmode) {
      const th = E.mTheta;
      ctx.strokeStyle = 'rgba(120,220,255,.7)'; ctx.setLineDash([4, 4]);
      ctx.beginPath(); ctx.moveTo(eng.cx, 4); ctx.lineTo(eng.cx + Math.sin(th) * set.depth * eng.scale, 4 + Math.cos(th) * set.depth * eng.scale); ctx.stroke();
      ctx.setLineDash([]);
    }
    // mesures
    for (const m of E.meas) if (m.where === 'b') drawCaliper(ctx, m, eng.cx, eng.scale, 0);
    if (E.drag && E.drag.where === 'b') drawCaliper(ctx, E.drag, eng.cx, eng.scale, 0);
    ctx.restore();
    void f; void G;
  }

  function drawCaliper(c, m, cx, s, oy) {
    c.strokeStyle = '#ffe26a'; c.fillStyle = '#ffe26a'; c.lineWidth = 1.2;
    const X = (p) => (m.where === 'b' ? cx + p[0] * s : p[0]);
    const Yp = (p) => (m.where === 'b' ? 4 + p[1] * s : p[1]) + oy;
    c.beginPath(); c.moveTo(X(m.a), Yp(m.a)); c.lineTo(X(m.b), Yp(m.b)); c.stroke();
    for (const p of [m.a, m.b]) { c.beginPath(); c.moveTo(X(p) - 5, Yp(p)); c.lineTo(X(p) + 5, Yp(p)); c.moveTo(X(p), Yp(p) - 5); c.lineTo(X(p), Yp(p) + 5); c.stroke(); }
    c.font = '12px system-ui';
    c.fillText(`${m.lab ? labOf(m.lab) + ' ' : ''}${fmt(m.cm, 2)} cm`, X(m.b) + 6, Yp(m.b) - 6);
  }

  function drawEcg() {
    const y0 = IH + 10, h = 70, win = 3;
    ctx.fillStyle = '#000'; ctx.fillRect(0, IH, W, cv.height - IH);
    ctx.strokeStyle = '#3ad16b'; ctx.lineWidth = 1.2; ctx.beginPath();
    const fsE = r.ecg_fs, t1 = E.t, t0 = t1 - win;
    for (let px = 0; px < W; px++) {
      const tt = t0 + (px / W) * win;
      const i = Math.round(tt * fsE);
      const val = i >= 0 && i < r.ecg.length ? r.ecg[i] : 0;
      const y = y0 + h * 0.6 - val * h * 0.28;
      if (px === 0) ctx.moveTo(px, y); else ctx.lineTo(px, y);
    }
    ctx.stroke();
    ctx.fillStyle = '#e0465a'; ctx.fillRect(W - 2, y0, 2, h);
  }

  // ---------------------------------------------- mode TM
  function mmColumn(tcol) {
    const G = frameGeometry(r, sc, tcol), f = probeFrame(sc, pose());
    if (set.flip) f.L = mul(f.L, -1);
    const NS = eng.NS, half = set.sector / 2 * D2R, dth = 2 * half / (eng.NL - 1);
    const thetas = [-2, -1, 0, 1, 2].map((k) => E.mTheta + k * dth);
    const re = new Float32Array(5 * NS), im = new Float32Array(5 * NS), t1 = new Float32Array(5 * NS), t2 = new Float32Array(5 * NS), env = new Float32Array(5 * NS);
    const lam = acquire(sc, G, f, set, tcol, thetas, re, im, NS, Math.floor(tcol * 997));
    psf(set, 5, NS, re, im, t1, t2, lam, dth);
    toGray(set, re, im, env, 5, NS);
    return env.subarray(2 * NS, 3 * NS);
  }
  function drawM(advanceTo) {
    const MW = mm.width, MH = mm.height - 50, sweep = 3;
    const colDt = sweep / MW;
    if (E.mLast === undefined || advanceTo < E.mLast || advanceTo - E.mLast > sweep) E.mLast = advanceTo - colDt;
    let n = 0;
    while (E.mLast + colDt <= advanceTo && n < 40) {
      E.mLast += colDt; n++;
      const col = Math.floor(((E.mLast % sweep) + sweep) % sweep / colDt) % MW;
      const env = mmColumn(E.mLast), NS = env.length, dd = mmBuf.data;
      for (let y = 0; y < MH; y++) {
        const g = env[Math.min(NS - 1, Math.floor((y / MH) * NS))] * 255;
        const p = 4 * (y * MW + col);
        dd[p] = g; dd[p + 1] = g; dd[p + 2] = Math.min(255, g + 4); dd[p + 3] = 255;
      }
      E.mmT[col] = E.mLast;
    }
    mctx.fillStyle = '#000'; mctx.fillRect(0, 0, MW, mm.height);
    mctx.putImageData(mmBuf, 0, 0);
    const cur = Math.floor(((E.mLast % sweep) + sweep) % sweep / colDt);
    mctx.fillStyle = '#e0465a'; mctx.fillRect(cur, 0, 2, MH);
    // ECG sous le TM
    mctx.strokeStyle = '#3ad16b'; mctx.beginPath();
    for (let px = 0; px < MW; px++) {
      const tt = E.mmT[px]; if (tt === undefined) continue;
      const i = Math.round(tt * r.ecg_fs), val = r.ecg[i] || 0;
      const y = MH + 30 - val * 14;
      if (px === 0 || E.mmT[px - 1] === undefined || Math.abs(E.mmT[px - 1] - tt) > 0.1) mctx.moveTo(px, y); else mctx.lineTo(px, y);
    }
    mctx.stroke();
    mctx.fillStyle = '#9fd3c7'; mctx.font = '12px system-ui'; mctx.fillText('Mode TM : 3 s par balayage', 8, 14);
    for (const m of E.meas) if (m.where === 'm') drawCaliper(mctx, m, 0, 0, 0);
    if (E.drag && E.drag.where === 'm') drawCaliper(mctx, E.drag, 0, 0, 0);
  }

  // ---------------------------------------------- schéma d'orientation (deux projections)
  function drawInset(f) {
    const c = $('#e-inset'), g = c.getContext('2d'), w = c.width, h = c.height, G = geomNow();
    g.clearRect(0, 0, w, h); g.fillStyle = '#f7f9fa'; g.fillRect(0, 0, w, h);
    const k = (h - 30) / (2 * sc.Y);
    const panels = [
      { ox: w * 0.25, lab: 'coupe transverse (vue caudale)', px: (p) => -p[0], py: (p) => -p[1], ax: sc.X },
      { ox: w * 0.73, lab: 'vue latérale droite', px: (p) => p[2], py: (p) => -p[1], ax: Math.max(sc.Y, 1) },
    ];
    for (const pn of panels) {
      const P = (p) => [pn.ox + pn.px(p) * k, h / 2 + 4 + pn.py(p) * k];
      g.strokeStyle = '#9aa7b0'; g.beginPath();
      if (pn === panels[0]) g.ellipse(pn.ox, h / 2 + 4, sc.X * k, sc.Y * k, 0, 0, 7);
      else g.rect(pn.ox - 0.42 * w * 0.5, h / 2 + 4 - sc.Y * k, 0.42 * w, 2 * sc.Y * k);
      g.stroke();
      if (pn === panels[1]) {
        g.strokeStyle = '#c9b8a6';
        for (const z of sc.ribs) { const a = P([0, sc.Y, z]); g.beginPath(); g.moveTo(a[0], a[1]); g.lineTo(a[0], h / 2 + 4 + sc.Y * k * 0.8); g.stroke(); }
      }
      // cœur : VG (axe et contour approximatif), OG
      const base = P(sc.origin), apex = P(fromHeart(sc, [0, 0, G.epiL])), la = P(fromHeart(sc, G.LAc));
      g.strokeStyle = '#c0392b'; g.lineWidth = G.epiD * k; g.lineCap = 'round'; g.globalAlpha = 0.35;
      g.beginPath(); g.moveTo(base[0], base[1]); g.lineTo(apex[0], apex[1]); g.stroke();
      g.globalAlpha = 0.35; g.fillStyle = '#2e6fd1'; g.beginPath(); g.arc(la[0], la[1], G.la[0] / 2 * k, 0, 7); g.fill();
      g.globalAlpha = 1; g.lineWidth = 1;
      // secteur de la sonde
      const half = set.sector / 2 * D2R;
      g.fillStyle = 'rgba(255,176,0,.35)'; g.strokeStyle = '#e08a00'; g.beginPath();
      const p0 = P(f.P); g.moveTo(p0[0], p0[1]);
      for (let a = -half; a <= half + 1e-6; a += half / 8) {
        const u = add(mul(f.b, Math.cos(a)), mul(f.L, Math.sin(a)));
        const q = P(add(f.P, mul(u, set.depth))); g.lineTo(q[0], q[1]);
      }
      g.closePath(); g.fill(); g.stroke();
      g.fillStyle = '#111'; g.beginPath(); g.arc(p0[0], p0[1], 3, 0, 7); g.fill();
      g.fillStyle = '#5b6b77'; g.font = '11px system-ui'; g.fillText(pn.lab, pn.ox - 70, 12);
    }
  }

  function updateScore(f) {
    let best = null;
    for (const vw of VIEWS) {
      const s = viewScore(sc, f, vw);
      if (!best || s.q > best.s.q) best = { vw, s };
    }
    const n = norm(cross(f.b, f.L)), lvTilt = Math.asin(Math.min(1, Math.abs(dot(sc.e3, n)))) / D2R;
    let html = best.s.q > 0.05
      ? `Coupe la plus proche : <b>${best.vw.name}</b>, qualité ${fmt(100 * best.s.q, 0)} % (plan à ${fmt(best.s.ang, 0)}°, cible à ${fmt(best.s.off, 1)} cm du plan)`
      : 'Aucune coupe de référence reconnue.';
    if (best.vw.lax && best.s.q > 0.05) html += `<br>Axe du VG hors du plan : ${fmt(lvTilt, 0)}° (raccourcissement apparent ≈ ${fmt(100 * (1 - Math.cos(lvTilt * D2R)), 0)} %)`;
    $('#e-score').innerHTML = html;
  }

  function renderMeas() {
    const rows = E.meas.filter((m) => m.lab).map((m) => `<tr><td>${labOf(m.lab)}</td><td>${fmt(m.cm, 2)} cm</td>${E.showTruth ? `<td>${truthOf(m.lab)}</td>` : ''}</tr>`);
    const get = (k) => { const x = E.meas.filter((m) => m.lab === k); return x.length ? x.reduce((a, m) => a + m.cm, 0) / x.length : null; };
    const dv = [];
    const lvd = get('lvidd'), lvs = get('lvids'), la = get('la'), ao = get('ao');
    if (lvd && lvs) dv.push(`FR = ${fmt(100 * (1 - lvs / lvd), 0)} %${E.showTruth ? ` (modèle ${fmt(100 * tr.fs, 0)} %)` : ''}`);
    if (lvd) dv.push(`LVIDdN = ${fmt(lvd / Math.pow(r.echo.bw, 0.294), 2)}${E.showTruth ? ` (modèle ${fmt(tr.lvidd_n, 2)})` : ''}`);
    if (la && ao) dv.push(`OG/Ao = ${fmt(la / ao, 2)}${E.showTruth ? ` (modèle ${fmt(tr.la_ao, 2)})` : ''}`);
    $('#e-mlist').innerHTML = (rows.length ? `<table class="ec-tab"><tr><th>Mesure</th><th>Vous</th>${E.showTruth ? '<th>Modèle</th>' : ''}</tr>${rows.join('')}</table>` : '')
      + (dv.length ? `<div class="note">${dv.join(' · ')}</div>` : '')
      + (E.showTruth ? `<div class="note">Modèle (coupe parfaite, moyenne de ${tr.n_beats} battements) : LVIDd ${fmt(tr.lvidd, 2)} cm, LVIDs ${fmt(tr.lvids, 2)} cm, paroi ${fmt(tr.wall_d, 2)}/${fmt(tr.wall_s, 2)} cm, Ao ${fmt(tr.ao, 2)} cm, OG ${fmt(tr.la_d, 2)} cm (fermeture aortique), volume de l'OG ${fmt(tr.la_min, 1)}–${fmt(tr.la_max, 1)} mL (${fmt(tr.la_max / r.echo.bw, 2)} mL/kg), FE ${fmt(100 * tr.ef, 0)} %.</div>` : '');
  }
  const truthOf = (k) => ({ lvidd: tr.lvidd, lvids: tr.lvids, ivsd: tr.wall_d, lvwd: tr.wall_d, ivss: tr.wall_s, lvws: tr.wall_s, la: tr.la_d, ao: tr.ao }[k] !== undefined
    ? fmt({ lvidd: tr.lvidd, lvids: tr.lvids, ivsd: tr.wall_d, lvwd: tr.wall_d, ivss: tr.wall_s, lvws: tr.wall_s, la: tr.la_d, ao: tr.ao }[k], 2) + ' cm' : '—');

  // ---------------------------------------------- boucle
  function tick(now) {
    if (!cv.isConnected) { window.removeEventListener('keydown', E.onKey); E.raf = null; return; }
    const dt = E.last ? Math.min(0.1, (now - E.last) / 1000) : 0;
    E.last = now;
    if (E.playing) { E.t = (E.t + dt * E.speed) % (dur - 0.05); dirty = true; }
    if (dirty) {
      const { f } = drawB();
      if (E.mmode) drawM(E.t);
      drawInset(f); updateScore(f);
      $('#e-time').textContent = `t = ${fmt(E.t, 3)} s`;
      dirty = !!E.playing;
    }
    E.raf = requestAnimationFrame(tick);
  }

  // ---------------------------------------------- commandes
  const bindPose = (id, key, d = 1) => {
    $('#' + id).oninput = (ev) => { E.pose[key] = +ev.target.value; $('#' + id + '-v').textContent = fmt(+ev.target.value, d) + (key === 'z' ? ' cm' : '°'); dirty = true; };
  };
  bindPose('e-psi', 'psi'); bindPose('e-z', 'z', 2); bindPose('e-tz', 'tiltZ'); bindPose('e-tc', 'tiltC'); bindPose('e-rot', 'rot', 0);
  $('#e-side').onchange = (ev) => { E.pose.side = ev.target.value; dirty = true; };
  const bindSet = (id, key, unit, d = 1, after) => {
    $('#' + id).oninput = (ev) => { set[key] = +ev.target.value; $('#' + id + '-v').textContent = fmt(set[key], d) + unit; if (after) after(); dirty = true; E.mLast = undefined; };
  };
  bindSet('e-freq', 'freq', ' MHz'); bindSet('e-depth', 'depth', ' cm'); bindSet('e-focus', 'focus', ' cm');
  bindSet('e-gain', 'gain', ' dB', 0); bindSet('e-dr', 'dr', ' dB', 0);
  [0, 1, 2].forEach((i) => { $('#e-tgc' + i).oninput = (ev) => { set.tgc[i] = +ev.target.value; $('#e-tgc' + i + '-v').textContent = set.tgc[i] + ' dB'; dirty = true; }; });
  const syncPoseUI = () => {
    for (const [id, key, d] of [['e-psi', 'psi', 1], ['e-z', 'z', 2], ['e-tz', 'tiltZ', 1], ['e-tc', 'tiltC', 1], ['e-rot', 'rot', 0]]) {
      $('#' + id).value = E.pose[key]; $('#' + id + '-v').textContent = fmt(E.pose[key], d) + (key === 'z' ? ' cm' : '°');
    }
    $('#e-side').value = E.pose.side;
  };
  v.querySelectorAll('[data-view]').forEach((b) => {
    b.onclick = () => { E.pose = solveView(sc, geomNow(), VIEWS[+b.dataset.view], E.t); syncPoseUI(); dirty = true; E.mLast = undefined; };
  });
  $('#e-play').onclick = () => { E.playing = !E.playing; $('#e-play').textContent = E.playing ? '❚❚' : '▶'; dirty = true; };
  v.querySelectorAll('[data-espeed]').forEach((b) => { b.onclick = () => { E.speed = +b.dataset.espeed; v.querySelectorAll('[data-espeed]').forEach((x) => x.classList.toggle('on', x === b)); }; });
  const step = (k) => { E.playing = false; $('#e-play').textContent = '▶'; E.t = Math.max(0, Math.min(dur - 0.05, E.t + k / 60)); dirty = true; };
  $('#e-prev').onclick = () => step(-1); $('#e-next').onclick = () => step(1);
  $('#e-mm').onchange = (ev) => { E.mmode = ev.target.checked; mm.hidden = !E.mmode; E.mLast = undefined; dirty = true; };
  $('#e-flip').onchange = (ev) => { set.flip = ev.target.checked; dirty = true; };
  $('#e-mclear').onclick = () => { E.meas = []; renderMeas(); dirty = true; };
  $('#e-truth').onclick = () => { E.showTruth = !E.showTruth; $('#e-truth').textContent = (E.showTruth ? 'Masquer' : 'Comparer au') + ' modèle'; renderMeas(); };

  // calipers et ligne TM
  const toCm = (ev) => {
    const b = cv.getBoundingClientRect(), px = (ev.clientX - b.left) * (cv.width / b.width), py = (ev.clientY - b.top) * (cv.height / b.height);
    return [(px - eng.cx) / eng.scale, (py - 4) / eng.scale];
  };
  cv.onpointerdown = (ev) => {
    const p = toCm(ev);
    if (E.mmode && ev.shiftKey || (E.mmode && E.playing)) { E.mTheta = Math.atan2(p[0], p[1]); E.mLast = undefined; dirty = true; return; }
    if (E.playing) return;
    E.drag = { where: 'b', a: p, b: p, cm: 0, lab: $('#e-mlab').value };
    cv.setPointerCapture(ev.pointerId);
  };
  cv.onpointermove = (ev) => { if (!E.drag || E.drag.where !== 'b') return; E.drag.b = toCm(ev); E.drag.cm = Math.hypot(E.drag.b[0] - E.drag.a[0], E.drag.b[1] - E.drag.a[1]); dirty = true; };
  cv.onpointerup = () => { if (E.drag && E.drag.where === 'b' && E.drag.cm > 0.05) E.meas.push(E.drag); E.drag = null; renderMeas(); dirty = true; };
  const mPx = (ev) => { const b = mm.getBoundingClientRect(); return [(ev.clientX - b.left) * (mm.width / b.width), (ev.clientY - b.top) * (mm.height / b.height)]; };
  mm.onpointerdown = (ev) => { if (E.playing) return; const p = mPx(ev); E.drag = { where: 'm', a: p, b: p, cm: 0, lab: $('#e-mlab').value }; mm.setPointerCapture(ev.pointerId); };
  mm.onpointermove = (ev) => { if (!E.drag || E.drag.where !== 'm') return; const p = mPx(ev); E.drag.b = [E.drag.a[0], p[1]]; E.drag.cm = Math.abs(p[1] - E.drag.a[1]) / (mm.height - 50) * set.depth; dirty = true; };
  mm.onpointerup = cv.onpointerup;

  // gyroscope du téléphone : rotation relative à la pose de calibration
  const Rm = (a, b, g) => { // W3C DeviceOrientation (Z-X'-Y'')
    const ca = Math.cos(a * D2R), sa = Math.sin(a * D2R), cb = Math.cos(b * D2R), sb = Math.sin(b * D2R), cg = Math.cos(g * D2R), sg = Math.sin(g * D2R);
    return [[ca * cg - sa * sb * sg, -cb * sa, ca * sg + cg * sa * sb], [cg * sa + ca * sb * sg, ca * cb, sa * sg - ca * cg * sb], [-cb * sg, sb, cb * cg]];
  };
  const mt = (A, x) => [A[0][0] * x[0] + A[1][0] * x[1] + A[2][0] * x[2], A[0][1] * x[0] + A[1][1] * x[1] + A[2][1] * x[2], A[0][2] * x[0] + A[1][2] * x[1] + A[2][2] * x[2]];
  const mv = (A, x) => [A[0][0] * x[0] + A[0][1] * x[1] + A[0][2] * x[2], A[1][0] * x[0] + A[1][1] * x[1] + A[1][2] * x[2], A[2][0] * x[0] + A[2][1] * x[1] + A[2][2] * x[2]];
  const onOri = (ev) => {
    if (ev.alpha === null) return;
    const R = Rm(ev.alpha, ev.beta, ev.gamma);
    if (!E.gyro.R0) { E.gyro.R0 = R; const f = probeFrame(sc, E.pose); E.gyro.b0 = f.b; E.gyro.L0 = f.L; E.gyro.P = f.P; return; }
    // axes du téléphone dans le repère de calibration : R0ᵀ·R·e
    const rel = (e) => mt(E.gyro.R0, mv(R, e));
    const M = (x) => add(add(mul(E.gyro.L0, x[0]), mul(E.gyro.b0, x[1])), mul(cross(E.gyro.L0, E.gyro.b0), x[2]));
    const b = norm(M(rel([0, 1, 0]))), L = norm(M(rel([1, 0, 0])));
    const pz = poseFrom(sc, sub(E.gyro.P, mul(probeFrame(sc, E.pose).n, 0.02)), b, L);
    E.pose.tiltZ = Math.max(-60, Math.min(60, pz.tiltZ)); E.pose.tiltC = Math.max(-60, Math.min(60, pz.tiltC)); E.pose.rot = pz.rot;
    syncPoseUI(); dirty = true;
  };
  E.gyro = E.gyro || {};
  $('#e-gyro').onclick = async () => {
    try {
      if (typeof DeviceOrientationEvent !== 'undefined' && typeof DeviceOrientationEvent.requestPermission === 'function') {
        const p = await DeviceOrientationEvent.requestPermission(); if (p !== 'granted') throw new Error('permission refusée');
      }
      if (E.gyro.on) { window.removeEventListener('deviceorientation', onOri); E.gyro = {}; $('#e-gyro-st').textContent = 'Gyroscope arrêté.'; $('#e-cal').hidden = true; return; }
      E.gyro = { on: true }; window.addEventListener('deviceorientation', onOri);
      $('#e-cal').hidden = false;
      $('#e-gyro-st').textContent = 'Gyroscope actif : la pose actuelle sert de référence. Inclinez le téléphone pour orienter le faisceau.';
    } catch (e) { $('#e-gyro-st').textContent = 'Gyroscope indisponible : ' + e.message + ' (page servie en HTTPS requise sur téléphone).'; }
  };
  $('#e-cal').onclick = () => { E.gyro.R0 = null; };

  E.onKey = (ev) => {
    if (ev.target && /INPUT|SELECT|TEXTAREA/.test(ev.target.tagName)) return;
    if (ev.code === 'Space') { ev.preventDefault(); $('#e-play').click(); }
    if (ev.code === 'ArrowLeft') { ev.preventDefault(); step(-1); }
    if (ev.code === 'ArrowRight') { ev.preventDefault(); step(1); }
  };
  window.addEventListener('keydown', E.onKey);
  if (E.mTheta === undefined) E.mTheta = 0;
  renderMeas();
  E.last = null;
  E.raf = requestAnimationFrame(tick);
}

const MEAS = [['lvidd', 'LVIDd (VG télédiastole)'], ['lvids', 'LVIDs (VG télésystole)'], ['ivsd', 'SIVd'], ['lvwd', 'PPVGd'],
  ['ivss', 'SIVs'], ['lvws', 'PPVGs'], ['la', 'OG (petit axe, fermeture Ao)'], ['ao', 'Ao (petit axe)'], ['', 'Autre (sans étiquette)']];
const labOf = (k) => (MEAS.find((m) => m[0] === k) || ['', ''])[1].split(' (')[0];
