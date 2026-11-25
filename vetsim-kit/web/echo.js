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
// Bruit de valeur lisse (trilinéaire) dans [-1, 1] : irrégularités de forme (trabéculations).
function vnoise(x, y, z) {
  const ix = Math.floor(x), iy = Math.floor(y), iz = Math.floor(z), fx = x - ix, fy = y - iy, fz = z - iz;
  const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy), sz = fz * fz * (3 - 2 * fz);
  const v = (a, b, c) => hash3(ix + a, iy + b, iz + c) / 2147483648 - 1;
  const l = (a, b, u) => a + (b - a) * u;
  return l(l(l(v(0, 0, 0), v(1, 0, 0), sx), l(v(0, 1, 0), v(1, 1, 0), sx), sy),
    l(l(v(0, 0, 1), v(1, 0, 1), sx), l(v(0, 1, 1), v(1, 1, 1), sx), sy), sz);
}
// Facteur local de rétrodiffusion posé par la classification (bords endocardiques, couches pariétales).
let BST = 1;
const TEX = 0.15; // cm : maille de la microstructure (densité locale des diffuseurs)
// Modulation lognormale de la rétrodiffusion, attachée au tissu (aspect granuleux hétérogène).
function texture(mx, my, mz) {
  const h = hash3(Math.floor(mx / TEX) + 7919, Math.floor(my / TEX), Math.floor(mz / TEX));
  const u1 = (h + 1) / 4294967297, u2 = (Math.imul(h ^ 0x27d4eb2d, 0x165667b1) >>> 0) / 4294967296;
  return Math.sqrt(-2 * Math.log(u1)) * Math.cos(2 * Math.PI * u2);
}
const TEX_SD = [0, 1.1, 0.3, 0.3, 0.9, 0.6, 1.6, 0, 0.3]; // écart-type de ln(rétrodiffusion) par tissu
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
    bw, X, Y, wall, keel: th.ventral_taper || 0, Xi: X - wall, Yi: Y - wall, Xr: X - 0.25 * wall, Yr: Y - 0.25 * wall,
    ribs, ribW, notchY: hz.lung_notch_y_frac * Y, notchYL: (hz.lung_notch_y_frac - 0.45) * Y, e1, e2, e3, origin, center,
    origin0: origin, epiLmax: Math.max(...E.series.lv_l.map((x, i) => x + E.series.lv_w[i])),
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
  // apex quasi immobile, base mobile (descente de l'anneau en systole) : l'origine du repère
  // cardiaque (centre de la base) avance vers l'apex de 85 % du raccourcissement
  const bs = 0.85 * (sc.epiLmax - epiL);
  sc.origin = add(sc.origin0, mul(sc.e3, bs));
  const lr = g.rv_length_fraction * epiL;
  const G = {
    d, l, w, epiD, epiL, rvt, lr, rvw: st.rv_wall, cosCov: Math.cos(g.rv_coverage_deg * D2R),
    // anneau mitral (repère cardiaque, plan de la base)
    M: [-0.18 * d, 0], rm: 0.5 * st.mitral_annulus_d, lfm: st.leaflet_l,
    am: Math.pow(at(S.mi_open), 0.7) * sc.geo.mitral_open_deg * D2R,
    // racine aortique
    A: [0.42 * d, 0.28 * d, -0.15], ao: norm([0.1, 0.25, -1]), rao: st.ao_d / 2, aoLen: 3.2 * st.ao_d,
    aa: Math.pow(at(S.ao_open), 0.7) * 80 * D2R,
    // tricuspide (base du croissant du VD)
    Tc: [epiD / 2 + rvt / 2, 0], rt: Math.max(0.3, 0.55 * rvt + 0.3), at: Math.pow(at(S.tr_open), 0.7) * sc.geo.tricuspid_open_deg * D2R,
    la, ra,
  };
  { const ax = G.A[0] - G.M[0], ay = G.A[1] - G.M[1], an = Math.hypot(ax, ay) || 1; G.aoDir = [ax / an, ay / an]; }
  G.pap = [-1, 1].map((sg) => {
    const ph = Math.PI + sg * 0.45, z = 0.58 * l, rIn = bulletR(d, l, z);
    const a = 0.16 * d, al = 0.2 * l;
    const c = [Math.cos(ph) * (rIn - 0.6 * a), Math.sin(ph) * (rIn - 0.6 * a), z];
    const tip = [c[0] * 0.92, c[1] * 0.92, z - al * 0.95];
    // extrémités libres des feuillets (dans le plan du pilier)
    const alM = Math.pow(at(S.mi_open), 0.7) * sc.geo.mitral_open_deg * D2R;
    const ch = [-0.35, 0, 0.35].map((dp) => {
      const pp = ph + dp, rr = 0.5 * st.mitral_annulus_d - st.leaflet_l * Math.cos(alM) * 0.9;
      return [-0.18 * d + Math.cos(pp) * rr, Math.sin(pp) * rr, st.leaflet_l * Math.sin(alM) * 0.9 + 0.05];
    });
    return { c, a, al, tip, ch };
  });
  G.LAc = [G.M[0] - 0.05 * la[0], G.M[1] - 0.1 * la[1], -(la[2] / 2 - 0.45)];
  G.RAc = [epiD / 2 + rvt * 0.6 + 0.15 * ra[0], -0.25 * ra[1], -(ra[2] / 2 - 0.25)];
  // sac péricardique : ellipsoïde englobante
  const top = Math.min(-la[2] + 0.25, -ra[2] + 0.25, -G.aoLen * 0.6);
  const bot = epiL + 0.25;
  G.sac = { c: [0.25 * (rvt + G.rvw), 0, (top + bot) / 2], a: [epiD / 2 + rvt + G.rvw + 0.55, Math.max(epiD / 2, la[1] * 0.75) + 0.6, (bot - top) / 2 + 0.25] };
  G.laW = 0.18; G.raW = 0.15; G.aoW = 0.16; G.vth = 0.09; G.vthM = 0.13; G.periW = 0.15; G.envW = 0.3;
  return G;
}

// ------------------------------------------------------------------ classification d'un point
const MAT = new Float64Array(3);
function classify(sc, G, x, y, z, h1, h2, h3, t) {
  x /= taper(sc, y); // thorax en carène : plus étroit ventralement
  const qo = (x / sc.X) ** 2 + (y / sc.Y) ** 2;
  MAT[0] = x; MAT[1] = y; MAT[2] = z;
  BST = 1; REGION = 0;
  if (qo > 1) return T.AIR;
  const qi = (x / sc.Xi) ** 2 + (y / sc.Yi) ** 2;
  if (qi > 1) {
    const qr = (x / sc.Xr) ** 2 + (y / sc.Yr) ** 2;
    if (qr <= 1) {
      for (const rz of sc.ribs) if (Math.abs(z - rz) < sc.ribW / 2) return T.BONE;
    }
    // couches de la paroi (u = 0 à la peau, 1 au fascia endothoracique) : peau, tissu sous-cutané,
    // muscles striés séparés par des fascias, graisse extrapleurale
    const u = (1 - Math.sqrt(qo)) / (1 - Math.sqrt(sc.Xi / sc.X)) ;
    const fas = (c, w) => Math.exp(-(((u - c) / w) ** 2));
    if (u < 0.05) { BST = 2.2; return T.WALL; }
    if (u < 0.22) { BST = 0.35 + 3 * fas(0.14, 0.015); return T.FAT; }
    BST = Math.max(0.05, 0.45 + 0.9 * vnoise(z * 6, u * 14, x * 2)) + 4 * fas(0.22, 0.012) + 3 * fas(0.55, 0.012) + 4 * fas(0.88, 0.015);
    // stries musculaires : diffuseurs allongés le long des fibres (crânio-caudales)
    MAT[2] = z * 0.25;
    return T.WALL;
  }
  const sac = G.sac;
  const sv = ((h1 - sac.c[0]) / sac.a[0]) ** 2 + ((h2 - sac.c[1]) / sac.a[1]) ** 2 + ((h3 - sac.c[2]) / sac.a[2]) ** 2;
  if (sv < 1) {
    const id = heartTissue(G, h1, h2, h3, t, 0);
    if (id) return id;
    // enveloppe : graisse épicardique dans les sillons, péricarde à la surface externe, puis poumon
    if (envelope(G, h1, h2, h3, G.envW)) {
      if (envelope(G, h1, h2, h3, G.envW - G.periW)) { matHeart(h1, h2, h3, 8); return T.FAT; }
      matHeart(h1, h2, h3, 9); BST = 2.5; return T.PERI;
    }
  }
  // incisures cardiaques : à droite (fenêtre parasternale), plus petite et plus ventrale à gauche (fenêtre apicale)
  if (z < sc.zLo || z > sc.zHi) return T.LUNG;
  if (x <= 0 ? y > sc.notchY : y > sc.notchYL) return T.LUNG;
  return T.FAT;
}

// Feuillet valvulaire en entonnoir : charnière au rayon `ra`, ouvert de l'angle `al` vers +s.
function leaflet(rho, s, ra, lf, al, thk) {
  if (rho > ra * 1.02) return false;
  const ca = Math.cos(al);
  if (rho < ra - lf * ca) return false;
  const sl = (ra - rho) * Math.tan(al);
  return Math.abs(s - sl) * ca < thk / 2;
}

function heartTissue(G, h1, h2, h3, t, g) {
  if (g > 0) return envelope(G, h1, h2, h3, g);
  // racine aortique et valve aortique
  const r1 = h1 - G.A[0], r2 = h2 - G.A[1], r3 = h3 - G.A[2];
  const sa = r1 * G.ao[0] + r2 * G.ao[1] + r3 * G.ao[2];
  const p1 = r1 - sa * G.ao[0], p2 = r2 - sa * G.ao[1], p3 = r3 - sa * G.ao[2];
  const rhoA = Math.sqrt(p1 * p1 + p2 * p2 + p3 * p3);
  if (sa > -0.6 && sa < G.aoLen && rhoA < G.rao + G.aoW) {
    if (sa > -0.25 && sa < G.rao * 1.2 && leaflet(rhoA, sa, G.rao, G.rao * 1.02, G.aa, G.vth)) { matHeart(h1, h2, h3, 0); return T.VALVE; }
    if (rhoA < G.rao) { matBlood(h1, h2, h3, t, RG.AO); return T.BLOOD; }
    if (sa > 0.05) { matHeart(h1, h2, h3, 0); return T.PERI; }
  }
  // valve mitrale (s'ouvre vers l'apex, +h3) et vestibule
  const m1 = h1 - G.M[0], m2 = h2 - G.M[1], rhoM = Math.sqrt(m1 * m1 + m2 * m2);
  if (rhoM < G.rm * 1.05 && h3 > -1.2 && h3 < G.lfm + 0.1) {
    const lfm = G.lfm * (1 + 0.32 * ((m1 * G.aoDir[0] + m2 * G.aoDir[1]) / Math.max(1e-6, rhoM)));
    if (leaflet(rhoM, h3, G.rm, lfm, G.am, G.vthM)) { matHeart(h1, h2, h3, 1); BST = 1.2; return T.VALVE; }
    // vestibule mitral : raccorde l'anneau à l'OG sans interstice
    if (h3 <= 0 && rhoM < G.rm * (1 + 0.25 * Math.min(1, -h3 / 1.2))) { matBlood(h1, h2, h3, t, RG.MV); return T.BLOOD; }
  }
  // valve tricuspide
  const t1 = h1 - G.Tc[0], t2 = h2 - G.Tc[1], rhoT = Math.sqrt(t1 * t1 + t2 * t2);
  if (rhoT < G.rt * 1.05 && h3 > -1.2 && h3 < G.rt + 0.1) {
    if (leaflet(rhoT, h3, G.rt, G.rt * 0.9, G.at, G.vth)) { matHeart(h1, h2, h3, 2); return T.VALVE; }
    if (h3 <= 0 && rhoT < G.rt) { matBlood(h1, h2, h3, t, RG.TV); return T.BLOOD; }
  }
  const rho = Math.sqrt(h1 * h1 + h2 * h2);
  // VG : cavité et myocarde (coordonnées matérielles normalisées : le speckle suit la paroi)
  if (h3 >= 0 && h3 <= G.epiL) {
    const ph = Math.atan2(h2, h1), zn = h3 / G.epiL;
    // trabéculations : endocarde irrégulier, plus marqué vers l'apex ; attaché au tissu (zn, ph)
    const tr = 1 + (0.03 + 0.07 * zn) * vnoise(zn * 9, Math.cos(ph) * 3 + 11, Math.sin(ph) * 3);
    const rin0 = bulletR(G.d, G.l, h3), rin = rin0 * tr, rout = bulletR(G.epiD, G.epiL, h3) * (1 + 0.035 * vnoise(zn * 4 + 5, Math.cos(ph) * 1.6, Math.sin(ph) * 1.6));
    if (rho < rin) {
      // piliers (paroi libre, à l'opposé du septum) et cordages vers les feuillets mitraux
      for (const pm of G.pap) {
        const q = ((h1 - pm.c[0]) / pm.a) ** 2 + ((h2 - pm.c[1]) / pm.a) ** 2 + ((h3 - pm.c[2]) / pm.al) ** 2;
        if (q < 1) { matHeart(h1 / G.d * 4, h2 / G.d * 4, zn * 8, 6); BST = q > 0.75 ? 1.8 : 1; return T.MYO; }
        for (const ch of pm.ch) if (segDist2(h1, h2, h3, pm.tip, ch) < 0.0016) { matHeart(h1, h2, h3, 7); BST = 0.8; return T.VALVE; }
      }
      matBlood(h1, h2, h3, t, RG.LV); return T.BLOOD;
    }
    if (rho < rout) {
      const nu = (rho - rin) / Math.max(1e-6, rout - rin);
      const rr = 2 + nu * 0.9, zz = zn * 8;
      MAT[0] = 1000 + rr * Math.cos(ph) * 1.6; MAT[1] = rr * Math.sin(ph) * 1.6; MAT[2] = zz;
      // endocarde et épicarde plus échogènes (fibres et tissu conjonctif)
      BST = 1 + 1.8 * Math.exp(-nu / 0.06) + 1.2 * Math.exp(-(1 - nu) / 0.05);
      return T.MYO;
    }
  }
  // VD : croissant et paroi libre
  if (h3 >= 0 && h3 <= G.lr + G.rvt + G.rvw && rho > 1e-6 && h1 / rho >= G.cosCov) {
    const ro = bulletR(G.epiD + 2 * G.rvt, G.lr + G.rvt, h3), rw = bulletR(G.epiD + 2 * G.rvt + 2 * G.rvw, G.lr + G.rvt + G.rvw, h3);
    if (rho < ro) { matBlood(h1, h2, h3, t, RG.RV); return T.BLOOD; }
    if (rho < rw) { matHeart(h1, h2, h3, 3); return T.MYO; }
  }
  // oreillettes (ellipsoïdes, paroi mince)
  for (const [c, a, wth, k] of [[G.LAc, G.la, G.laW, 4], [G.RAc, G.ra, G.raW, 5]]) {
    const dx = (h1 - c[0]) / (a[1] / 2), dy = (h2 - c[1]) / (a[0] / 2), dz = (h3 - c[2]) / (a[2] / 2);
    // forme irrégulière (auricule, veines pulmonaires) : rayon modulé par un bruit lisse de la direction
    const rn = Math.sqrt(dx * dx + dy * dy + dz * dz) || 1;
    const q = (dx * dx + dy * dy + dz * dz) / (1 + 0.12 * vnoise(dx / rn * 1.5 + 3 * k, dy / rn * 1.5, dz / rn * 1.5)) ** 2;
    if (q < 1) { matBlood(h1, h2, h3, t, k === 4 ? RG.LA : RG.RA); return T.BLOOD; }
    const g = 1 + wth / ((a[0] + a[1] + a[2]) / 6);
    if (q < g * g) { matHeart(h1, h2, h3, k); return T.MYO; }
  }
  return 0;
}
// Enveloppe des cavités dilatée de `g` (péricarde).
function envelope(G, h1, h2, h3, g) {
  const rho = Math.sqrt(h1 * h1 + h2 * h2);
  if (h3 >= -g && h3 <= G.epiL + g && rho < bulletR(G.epiD + 2 * g, G.epiL + g, Math.max(0, h3))) return true;
  if (h3 >= -g && h3 <= G.lr + G.rvt + G.rvw + g && rho > 1e-6 && h1 / rho >= G.cosCov - 0.15
    && rho < bulletR(G.epiD + 2 * (G.rvt + G.rvw + g), G.lr + G.rvt + G.rvw + g, Math.max(0, h3))) return true;
  for (const [c, a, wth] of [[G.LAc, G.la, G.laW], [G.RAc, G.ra, G.raW]]) {
    const q = ((h1 - c[0]) / (a[1] / 2 + wth + g)) ** 2 + ((h2 - c[1]) / (a[0] / 2 + wth + g)) ** 2 + ((h3 - c[2]) / (a[2] / 2 + wth + g)) ** 2;
    if (q < 1) return true;
  }
  const r1 = h1 - G.A[0], r2 = h2 - G.A[1], r3 = h3 - G.A[2];
  const sa = r1 * G.ao[0] + r2 * G.ao[1] + r3 * G.ao[2];
  if (sa > -0.6 && sa < G.aoLen * 0.7) {
    const p1 = r1 - sa * G.ao[0], p2 = r2 - sa * G.ao[1], p3 = r3 - sa * G.ao[2];
    if (Math.sqrt(p1 * p1 + p2 * p2 + p3 * p3) < G.rao + G.aoW + g) return true;
  }
  return false;
}
// Carré de la distance du point au segment [a, b].
function segDist2(x, y, z, a, b) {
  const ux = b[0] - a[0], uy = b[1] - a[1], uz = b[2] - a[2];
  const L2 = ux * ux + uy * uy + uz * uz || 1e-9;
  const t = Math.max(0, Math.min(1, ((x - a[0]) * ux + (y - a[1]) * uy + (z - a[2]) * uz) / L2));
  const dx = x - a[0] - t * ux, dy = y - a[1] - t * uy, dz = z - a[2] - t * uz;
  return dx * dx + dy * dy + dz * dz;
}
function matHeart(h1, h2, h3, k) { MAT[0] = 2000 + 100 * k + h1; MAT[1] = h2; MAT[2] = h3; }
function matBlood(h1, h2, h3, t, rg) { MAT[0] = 5000 + h1 + 7.3 * t; MAT[1] = h2 - 3.1 * t; MAT[2] = h3 + 5.7 * t; REGION = rg; }
// Région du sang au dernier point classé (pour le champ de vitesses du Doppler).
const RG = { NONE: 0, LV: 1, RV: 2, LA: 3, RA: 4, AO: 5, MV: 6, TV: 7 };
let REGION = 0;

// ------------------------------------------------------------------ sonde
function probeFrame(sc, pose) {
  const side = pose.side === 'left' ? 1 : -1, psi = pose.psi * D2R;
  const P = surfacePoint(sc, side, psi, sc.center[2] + pose.z);
  const n = wallNormal(sc, P); // normale entrante
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
  const psi = Math.atan2(P[1] / sc.Y, (sx * P[0]) / taper(sc, P[1]) / sc.X) / D2R;
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
// Largeur relative du thorax à la hauteur y (1 dorsalement, 1 − κ au sternum).
function taper(sc, y) { return 1 - sc.keel * (1 - Math.max(-1, Math.min(1, y / sc.Y))) / 2; }
// Normale entrante de la paroi externe au point P.
function wallNormal(sc, P) {
  const tp = taper(sc, P[1]), xs = P[0] / tp;
  const gx = 2 * xs / (sc.X * sc.X) / tp;
  const gy = 2 * P[1] / (sc.Y * sc.Y) + 2 * xs * xs / (sc.X * sc.X) * (sc.keel / (2 * sc.Y)) / tp;
  return norm([-gx, -gy, 0]);
}
function surfacePoint(sc, sx, psiRad, z) { const y = sc.Y * Math.sin(psiRad); return [sx * sc.X * Math.cos(psiRad) * taper(sc, y), y, z]; }
const inside = (sc, p) => (p[0] / taper(sc, p[1]) / sc.X) ** 2 + (p[1] / sc.Y) ** 2 < 1;
function hitWall(sc, p, u) {
  let a = 0, b = 0.5;
  while (inside(sc, add(p, mul(u, b))) && b < 200) { a = b; b *= 2; }
  for (let i = 0; i < 50; i++) { const m = (a + b) / 2; if (inside(sc, add(p, mul(u, m)))) a = m; else b = m; }
  return add(p, mul(u, (a + b) / 2));
}
function hitWallOld(sc, p, u) {
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
    { id: 'rplax4', name: 'Parasternale droite grand axe 4 cavités', side: 'right', target: lvMid, normal: sc.e2, beam: mul(sc.e1, -1), lat: mul(sc.e3, -1), lax: true,
      keys: [fromHeart(sc, [0, 0, G.epiL * 0.95]), laMid, fromHeart(sc, [G.M[0], 0, 0])] },
    { id: 'rplax5', name: 'Parasternale droite grand axe, chambre de chasse', side: 'right', target: fromHeart(sc, [0.2 * G.d, 0.14 * G.d, G.l * 0.35]),
      normal: norm(cross(sc.e3, dirFromHeart(sc, [0.83, 0.55, 0]))), beam: mul(dirFromHeart(sc, [0.83, 0.55, 0]), -1), lat: mul(sc.e3, -1), lax: true },
    { id: 'rpsa', name: 'Parasternale droite petit axe (piliers)', side: 'right', target: fromHeart(sc, [0, 0, G.l * 0.55]), normal: sc.e3, beam: mul(sc.e1, -1), lat: sc.e2 },
    { id: 'rpsab', name: 'Parasternale droite petit axe, base (Ao/OG)', side: 'right', target: mul(add(aoAtLa, laMid), 0.5), normal: aoAxis, beam: mul(sc.e1, -1), lat: sc.e2 },
    { id: 'lap4', name: 'Apicale gauche 4 cavités', side: 'left', target: fromHeart(sc, [0, 0, G.l * 0.3]), normal: sc.e2, beam: mul(sc.e3, -1), lat: sc.e1, lax: true,
      keys: [laMid, fromHeart(sc, [0, 0, G.epiL * 0.9])] },
  ];
}

// Pose qui réalise une coupe de référence : recherche du point de contact sur toute la paroi du
// côté de la fenêtre (faisceau vers la cible, dans le plan de référence, sans poumon ni côte).
function solveView(sc, G, v, t) {
  const sx = v.side === 'left' ? 1 : -1;
  let best = null;
  for (let psi = -84; psi <= 40; psi += 3) {
    for (let z = -9; z <= 9; z += 0.5) {
      const P0 = surfacePoint(sc, sx, psi * D2R, sc.center[2] + z);
      const n = wallNormal(sc, P0);
      const P = add(P0, mul(n, 0.02));
      const b = norm(sub(v.target, P));
      if (dot(b, n) < 0.35) continue; // angulation au-delà de ~70° : impossible
      const off = Math.abs(dot(b, v.normal)); // le faisceau doit être dans le plan de référence
      if (off > 0.35) continue;
      const along = dot(b, v.beam); // sens d'insonation de la coupe de référence
      let L = norm(cross(v.normal, b));
      if (dot(L, v.lat) < 0) L = mul(L, -1);
      const dist = Math.hypot(...sub(v.target, P));
      let bad = 0;
      for (const ang of [-0.45, -0.22, 0, 0.22, 0.45]) {
        const u = norm(add(mul(b, Math.cos(ang)), mul(L, Math.sin(ang))));
        for (let s2 = 0.1; s2 < dist; s2 += 0.15) {
          const p = add(P, mul(u, s2)), h = toHeart(sc, p);
          const id = classify(sc, G, p[0], p[1], p[2], h[0], h[1], h[2], t);
          if (id === T.LUNG || id === T.BONE) { bad += 1; break; }
        }
      }
      let miss = 0;
      for (const kp of v.keys || []) {
        const q = sub(kp, P), zb = dot(q, b), xl = dot(q, L);
        if (zb <= 0 || Math.abs(Math.atan2(xl, zb)) > 36 * D2R || Math.hypot(zb, xl) > 9.5) miss += 1;
      }
      let best1 = null;
      // orientation dans le plan : la cible peut être décalée de l'axe pour cadrer les repères
      for (const tilt of [-20, -10, 0, 10, 20]) {
        const bb = norm(rotAbout(b, v.normal, tilt * D2R));
        let LL = norm(cross(v.normal, bb)); if (dot(LL, v.lat) < 0) LL = mul(LL, -1);
        let m2 = 0;
        for (const kp of v.keys || []) {
          const q = sub(kp, P), zb = dot(q, bb), xl = dot(q, LL);
          if (zb <= 0 || Math.abs(Math.atan2(xl, zb)) > 36 * D2R || Math.hypot(zb, xl) > 9.5) m2 += 1;
        }
        if (!best1 || m2 < best1.m) best1 = { m: m2, b: bb, L: LL, tilt };
      }
      void miss;
      const score = bad * 10 + off * 40 + (1 - along) * 8 + dist * 0.2 + best1.m * 6 + Math.abs(best1.tilt) * 0.03;
      if (!best || score < best.s) best = { s: score, P, b: best1.b, L: best1.L };
    }
  }
  return poseFrom(sc, sub(best.P, mul(wallNormal(sc, best.P), 0.02)), best.b, best.L);
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
function acquire(sc, G, f, set, t, thetas, re, im, NS, seedT, ids) {
  const dr = set.depth / NS, fMHz = set.freq, tis = sc.tis;
  // imagerie harmonique : émission à f, réception à 2f ; atténuation à f à l'aller et 2f au retour
  const fAtt = set.harmonic ? 1.5 * fMHz : fMHz, fRx = set.harmonic ? 2 * fMHz : fMHz;
  const lam = 0.154 / fRx; // cm
  const hP = toHeart(sc, f.P);
  for (let li = 0; li < thetas.length; li++) {
    const th = thetas[li], c = Math.cos(th), s = Math.sin(th);
    const u = [f.b[0] * c + f.L[0] * s, f.b[1] * c + f.L[1] * s, f.b[2] * c + f.L[2] * s];
    const hu = [dot(u, sc.e1), dot(u, sc.e2), dot(u, sc.e3)];
    let att = 0, trans = 1, prev = T.WALL, pleura = -1, pleuraA = 0, trans0 = 0;
    const base = li * NS;
    for (let k = 0; k < NS; k++) {
      const rr = (k + 0.5) * dr;
      const x = f.P[0] + u[0] * rr, y = f.P[1] + u[1] * rr, z = f.P[2] + u[2] * rr;
      const id = classify(sc, G, x, y, z, hP[0] + hu[0] * rr, hP[1] + hu[1] * rr, hP[2] + hu[2] * rr, t);
      const tp = tis[id];
      let ar = 0, ai = 0;
      if (id !== T.AIR && trans > 1e-6) {
        // aller-retour : 2 × att/20 dB ; l'harmonique se forme avec la propagation (faible en surface)
        const amp = trans * Math.pow(10, -att / 10) * (set.harmonic ? 1 - 0.75 * Math.exp(-rr / 1.2) : 1);
        scatter(MAT[0], MAT[1], MAT[2]);
        const bs = Math.sqrt(tp.backscatter * BST * Math.exp(TEX_SD[id] * texture(MAT[0], MAT[1], MAT[2]) - TEX_SD[id] * TEX_SD[id] / 2)) * amp;
        ar = bs * SPK[0]; ai = bs * SPK[1];
        if (id !== prev) {
          const z1 = tis[prev].z, z2 = tp.z, R = ((z2 - z1) / (z2 + z1)) ** 2;
          const sp = 2.2 * Math.sqrt(R) * amp;
          ar += sp; ai += sp * 0.3;
          trans *= 1 - R;
          if (id === T.LUNG && pleura < 0) { pleura = k; pleuraA = sp; trans0 = amp; }
        }
        att += tp.alpha_db_cm_mhz * fAtt * dr;
      } else if (id === T.AIR) trans = 0;
      // surface pulmonaire : ligne pleurale brillante puis zone texturée qui s'éteint (réverbérations
      // dans les alvéoles sous-pleurales), indépendamment de la transmission dans le poumon
      if (id === T.LUNG && pleura >= 0) {
        const dd = (k - pleura) * dr;
        if (dd < 2.5) {
          scatter(x * 3.1 + 900, y * 3.1, z * 0.4 + 37 * Math.floor(t * 2));
          const pa = trans0 * (1.8 * Math.exp(-dd / 0.12) + 0.2 * Math.exp(-dd / 0.6));
          ar += pa * SPK[0]; ai += pa * SPK[1];
        }
      }
      prev = id;
      if (ids) ids[base + k] = id;
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
    // bruit électronique et halo de réverbérations pariétales (clutter du champ proche,
    // décroissant avec la profondeur ; plus faible en harmonique)
    const hz = set.harmonic ? 0.012 : 0.03;
    for (let k = 0; k < NS; k++) {
      const rr = (k + 0.5) * dr;
      const h = hash3(li + seedT * 131, k, 77), g = hash3(li >> 1, k >> 2, 1201 + (seedT >> 3));
      const cl = hz * Math.exp(-rr / 2.2);
      re[base + k] += ((h & 0xffff) / 65536 - 0.5) * 2.2e-5 + cl * ((g & 0xffff) / 65536 - 0.5);
      im[base + k] += ((h >>> 16) / 65536 - 0.5) * 2.2e-5 + cl * ((g >>> 16) / 65536 - 0.5);
    }
  }
  return lam;
}

// Réponse impulsionnelle : axiale (≈ 1 longueur d'onde) puis latérale (faisceau focalisé).
function psf(set, NL, NS, re, im, tr, ti, lam, dth) {
  const dr = set.depth / NS;
  // impulsion d'environ 2 cycles : résolution axiale ≈ λ (FWHM)
  const sa = Math.max(0.6, (lam / 2.355) / dr), ra = Math.ceil(2.5 * sa);
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
    // lobes secondaires de l'ouverture : piédestal à −28 dB sur ±3 largeurs de faisceau
    // (voile des cavités au voisinage des structures brillantes)
    const rw = Math.min(NL - 1, Math.ceil(3 * sl) + 4), sideA = 0.04;
    let cr = 0, ci = 0;
    const pr = new Float64Array(NL + 1), pi = new Float64Array(NL + 1);
    for (let l = 0; l < NL; l++) { cr += tr[l * NS + k]; ci += ti[l * NS + k]; pr[l + 1] = cr; pi[l + 1] = ci; }
    for (let l = 0; l < NL; l++) {
      const a = Math.max(0, l - rw), b = Math.min(NL, l + rw + 1), n = b - a;
      re[l * NS + k] += sideA * (pr[b] - pr[a]) / Math.sqrt(n);
      im[l * NS + k] += sideA * (pi[b] - pi[a]) / Math.sqrt(n);
    }
  }
}

function tgcAt(set, rr) {
  const u = rr / set.depth, g = set.tgc;
  const v = u < 0.5 ? g[0] + (g[1] - g[0]) * (u / 0.5) : g[1] + (g[2] - g[1]) * ((u - 0.5) / 0.5);
  return v + 2 * 0.35 * set.freq * rr; // compensation de l'atténuation moyenne d'un trajet mixte sang/tissu
}

function toGray(set, re, im, env, NL, NS) {
  const dr = set.depth / NS, ref = 20 * Math.log10(Math.sqrt(0.3));
  // réduction de speckle légère (comme les filtres des échographes) : moyenne locale 3 × 3 de
  // l'intensité, mêlée à l'intensité brute ; comble les trous profonds du speckle de Rayleigh
  const P = new Float32Array(NL * NS);
  for (let i = 0; i < P.length; i++) P[i] = re[i] * re[i] + im[i] * im[i];
  const mix = set.srf ?? 0.5;
  for (let k = 0; k < NS; k++) {
    const add0 = set.gain + tgcAt(set, (k + 0.5) * dr) - ref;
    for (let l = 0; l < NL; l++) {
      const i = l * NS + k;
      let m = 0, n = 0;
      for (let dl = -1; dl <= 1; dl++) {
        const ll = l + dl; if (ll < 0 || ll >= NL) continue;
        for (let dk = -1; dk <= 1; dk++) { const kk = k + dk; if (kk < 0 || kk >= NS) continue; m += P[ll * NS + kk]; n++; }
      }
      const pw = (1 - mix) * P[i] + mix * m / n;
      const db = 10 * Math.log10(pw + 1e-24) + add0;
      const x = Math.min(1, Math.max(0, (db + set.dr) / set.dr - set.offset));
      env[i] = Math.pow(x, set.gamma);
    }
  }
}

// ------------------------------------------------------------------ Doppler
// Champ de vitesses du sang construit à partir des débits et des vitesses du moteur (API
// `echo.doppler`) : jets turbulents libres en aval des orifices (cône potentiel puis
// décroissance Uc = U0·B·d0/x, demi-largeur r½ = S·x ; Pope 2000), convergence hémisphérique
// en amont (v = Q / 2πr² : la zone de convergence, base de la PISA, en découle), écoulement en
// bloc dans l'aorte. Vitesses en cm/s, repère cardiaque.
const C_CM_S = 154000;
function dopplerFrame(r, t) {
  const D = r.echo.doppler.series, S = r.series, n = D.v_mi.length;
  const x = Math.min(Math.max(t * r.fs, 0), n - 1.001), i = Math.floor(x), f = x - i;
  const at = (a) => a[i] + (a[i + 1] - a[i]) * f;
  const o = {};
  for (const k of ['v_mi', 'a_mi', 'v_ao', 'v_tr', 'a_tv', 'v_pu', 'v_mr', 'eroa_mr', 'v_trj', 'eroa_tr', 'v_ar']) o[k] = at(D[k]);
  for (const k of ['q_mi', 'q_ao', 'q_tr', 'q_pu']) o[k] = at(S[k]);
  return o;
}
const FLOW = new Float64Array(4); // vx, vy, vz (repère cardiaque), écart-type turbulent
function addJet(p, o, ax, U0, d0, geo) {
  if (U0 <= 0 || d0 <= 0) return;
  const r0 = p[0] - o[0], r1 = p[1] - o[1], r2 = p[2] - o[2];
  const x = r0 * ax[0] + r1 * ax[1] + r2 * ax[2];
  if (x <= 0) return;
  const rho2 = Math.max(0, r0 * r0 + r1 * r1 + r2 * r2 - x * x);
  const core = geo.jet_decay_b * d0;
  const Uc = x < core ? U0 : U0 * core / x, rh = Math.max(d0 / 2, geo.jet_spread_rate * x);
  const u = Uc * Math.exp(-0.693 * rho2 / (rh * rh));
  FLOW[0] += u * ax[0]; FLOW[1] += u * ax[1]; FLOW[2] += u * ax[2];
  FLOW[3] = Math.max(FLOW[3], (x < core ? 0.04 : 0.22) * u);
}
function addSink(p, o, ax, Q, r0, cap) {
  if (Q <= 0) return;
  const d0 = p[0] - o[0], d1 = p[1] - o[1], d2 = p[2] - o[2];
  if (d0 * ax[0] + d1 * ax[1] + d2 * ax[2] > 0) return; // seulement en amont
  const rr = Math.sqrt(d0 * d0 + d1 * d1 + d2 * d2) || 1e-6;
  const v = Math.min(cap, Q / (2 * Math.PI * Math.max(r0, rr) ** 2));
  FLOW[0] -= v * d0 / rr; FLOW[1] -= v * d1 / rr; FLOW[2] -= v * d2 / rr;
}
function flowAt(G, Dd, geo, h, region) {
  FLOW[0] = FLOW[1] = FLOW[2] = 0; FLOW[3] = 2;
  const tipM = [G.M[0], G.M[1], G.lfm * Math.sin(G.am)], orM = [G.M[0], G.M[1], 0];
  const tipT = [G.Tc[0], G.Tc[1], G.rt * 0.9 * Math.sin(G.at)], orT = [G.Tc[0], G.Tc[1], 0];
  const dE = 2 * Math.sqrt(Dd.a_mi / Math.PI), dT = 2 * Math.sqrt(Dd.a_tv / Math.PI);
  const dMR = 2 * Math.sqrt(Math.max(0, Dd.eroa_mr) / Math.PI), dTR = 2 * Math.sqrt(Math.max(0, Dd.eroa_tr) / Math.PI);
  const up = [0, 0, 1], down = [0, 0, -1], negAo = [-G.ao[0], -G.ao[1], -G.ao[2]];
  switch (region) {
    case RG.LV:
      addJet(h, tipM, up, Dd.v_mi, dE, geo);
      addSink(h, G.A, G.ao, Dd.q_ao, G.rao, Dd.v_ao);
      if (Dd.v_mr > 0) addSink(h, orM, down, -Dd.q_mi, dMR / 2, Dd.v_mr);
      if (Dd.v_ar > 0) addJet(h, G.A, negAo, Dd.v_ar, 0.3, geo);
      break;
    case RG.AO: {
      const r0 = h[0] - G.A[0], r1 = h[1] - G.A[1], r2 = h[2] - G.A[2];
      const sa = r0 * G.ao[0] + r1 * G.ao[1] + r2 * G.ao[2];
      const rho = Math.sqrt(Math.max(0, r0 * r0 + r1 * r1 + r2 * r2 - sa * sa)) / G.rao;
      const u = Dd.v_ao * (1 - 0.25 * rho * rho) / 0.875;
      FLOW[0] = u * G.ao[0]; FLOW[1] = u * G.ao[1]; FLOW[2] = u * G.ao[2]; FLOW[3] = 0.03 * u + 2;
      break;
    }
    case RG.LA: case RG.MV:
      addSink(h, orM, up, Dd.q_mi, G.rm, Dd.v_mi);
      if (Dd.v_mr > 0) addJet(h, orM, down, Dd.v_mr, dMR, geo);
      break;
    case RG.RV:
      addJet(h, tipT, up, Dd.v_tr, dT, geo);
      if (Dd.v_trj > 0) addSink(h, orT, down, -Dd.q_tr, dTR / 2, Dd.v_trj);
      break;
    case RG.RA: case RG.TV:
      addSink(h, orT, up, Dd.q_tr, G.rt, Dd.v_tr);
      if (Dd.v_trj > 0) addJet(h, orT, down, Dd.v_trj, dTR, geo);
      break;
    default: break;
  }
}
// Vitesse Doppler (cm/s, positive vers la sonde) au point p du faisceau de direction u.
function dopplerAt(sc, G, Dd, geo, p, u, t) {
  const h = toHeart(sc, p);
  const id = classify(sc, G, p[0], p[1], p[2], h[0], h[1], h[2], t);
  if (id !== T.BLOOD) return null;
  flowAt(G, Dd, geo, h, REGION);
  const vb0 = FLOW[0] * sc.e1[0] + FLOW[1] * sc.e2[0] + FLOW[2] * sc.e3[0];
  const vb1 = FLOW[0] * sc.e1[1] + FLOW[1] * sc.e2[1] + FLOW[2] * sc.e3[1];
  const vb2 = FLOW[0] * sc.e1[2] + FLOW[1] * sc.e2[2] + FLOW[2] * sc.e3[2];
  return { v: -(vb0 * u[0] + vb1 * u[1] + vb2 * u[2]), s: FLOW[3], speed: Math.hypot(FLOW[0], FLOW[1], FLOW[2]), dir: [vb0, vb1, vb2] };
}
const dopFreq = (set) => (set.harmonic ? 1.6 * set.freq : set.freq); // MHz, émission Doppler
// Limite de Nyquist (cm/s) pour une profondeur maximale d'échantillonnage `dmax` (cm).
const nyquistMax = (set, dmax) => C_CM_S * (C_CM_S / (2 * dmax)) / (4 * dopFreq(set) * 1e6);
let SEED = 1;
const gauss = () => { SEED = (Math.imul(SEED, 1664525) + 1013904223) >>> 0; const u1 = (SEED + 1) / 4294967297; SEED = (Math.imul(SEED, 1664525) + 1013904223) >>> 0; return Math.sqrt(-2 * Math.log(u1)) * Math.cos(2 * Math.PI * SEED / 4294967296); };
const wrap = (v, vn) => { const w = 2 * vn; return ((((v + vn) % w) + w) % w) - vn; };

// Doppler couleur dans la boîte : vitesse moyenne estimée (bruit, turbulence) et repliement.
function colorDoppler(sc, G, Dd, geo, f, set, t, box, eng) {
  const NS = eng.NS, dr = set.depth / NS, half = set.sector / 2 * D2R, dth = 2 * half / (eng.NL - 1);
  const l0 = Math.max(0, Math.round((box.th0 + half) / dth)), l1 = Math.min(eng.NL - 1, Math.round((box.th1 + half) / dth));
  const s0 = Math.max(0, Math.round(box.r0 / dr)), s1 = Math.min(NS - 1, Math.round(box.r1 / dr));
  const LS = 2, SS = 4, nl = Math.floor((l1 - l0) / LS) + 1, ns = Math.floor((s1 - s0) / SS) + 1;
  const vn = Math.min(set.cScale, nyquistMax(set, box.r1));
  const vel = new Float32Array(nl * ns).fill(NaN), vr = new Float32Array(nl * ns);
  for (let a = 0; a < nl; a++) {
    const th = -half + (l0 + a * LS) * dth, c = Math.cos(th), s = Math.sin(th);
    const u = [f.b[0] * c + f.L[0] * s, f.b[1] * c + f.L[1] * s, f.b[2] * c + f.L[2] * s];
    for (let k = 0; k < ns; k++) {
      const rr = (s0 + k * SS + 0.5) * dr;
      const q = dopplerAt(sc, G, Dd, geo, [f.P[0] + u[0] * rr, f.P[1] + u[1] * rr, f.P[2] + u[2] * rr], u, t);
      if (!q) continue;
      const est = q.v + q.s * gauss() + 0.02 * vn * gauss();
      if (Math.abs(q.v) < set.wallFilter * vn && q.s < 0.3 * vn) continue; // filtre de paroi
      vel[a * ns + k] = wrap(est, vn) / vn;
      vr[a * ns + k] = Math.min(1, q.s / (0.5 * vn));
    }
  }
  return { l0, s0, LS, SS, nl, ns, vel, vr, vn };
}
function colorOf(v, vr) {
  const a = Math.abs(v);
  let r, g, b;
  if (v >= 0) { r = Math.min(1, 0.35 + 1.1 * a); g = Math.max(0, 1.5 * a - 0.55); b = 0; } else { b = Math.min(1, 0.4 + 1.1 * a); g = Math.max(0, 1.5 * a - 0.55); r = 0; }
  g = Math.min(1, g + 0.7 * vr);
  return [255 * r, 255 * g, 255 * b];
}

// Une colonne du spectre (PW : volume d'échantillonnage ; CW : toute la ligne).
function spectrumColumn(sc, r, set, pose, gate, tc, mode, nb, vmax) {
  const G = frameGeometry(r, sc, tc), Dd = dopplerFrame(r, tc), geo = r.echo.geometry, f = probeFrame(sc, pose);
  if (set.flip) f.L = mul(f.L, -1);
  const c = Math.cos(gate.theta), s = Math.sin(gate.theta);
  const u = [f.b[0] * c + f.L[0] * s, f.b[1] * c + f.L[1] * s, f.b[2] * c + f.L[2] * s];
  const per = norm(cross(cross(f.b, f.L), u));
  const hist = new Float32Array(nb);
  const vn = mode === 'pw' ? Math.min(nyquistMax(set, gate.r), set.pwScale) : vmax;
  const N = mode === 'pw' ? 28 : 70;
  for (let i = 0; i < N; i++) {
    const rr = mode === 'pw' ? gate.r + (Math.random() - 0.5) * set.gateLen : 0.4 + Math.random() * (set.depth - 0.4);
    const lat = (Math.random() - 0.5) * (mode === 'pw' ? 0.25 : 0.2);
    const p = add(add(f.P, mul(u, rr)), mul(per, lat));
    const q = dopplerAt(sc, G, Dd, geo, p, u, tc);
    if (!q) continue;
    for (let j = 0; j < 4; j++) {
      let v = q.v + q.s * gauss();
      v = mode === 'pw' ? wrap(v, vn) : Math.max(-vmax, Math.min(vmax, v));
      const sb = 0.05 * Math.abs(v) + 2; // élargissement spectral (ouverture)
      const bc = (v + vn) / (2 * vn) * (nb - 1), bw = sb / (2 * vn) * (nb - 1);
      for (let k = Math.max(0, Math.floor(bc - 2 * bw)); k <= Math.min(nb - 1, Math.ceil(bc + 2 * bw)); k++) hist[k] += Math.exp(-0.5 * ((k - bc) / Math.max(0.5, bw)) ** 2);
    }
  }
  return { hist, vn };
}

// ------------------------------------------------------------------ montage
export function mountEcho(v, r, E, ui) {
  if (E.raf) cancelAnimationFrame(E.raf);
  if (typeof window !== 'undefined') window.__echo = E; // accès pour les tests de l'interface
  if (E.onKey) window.removeEventListener('keydown', E.onKey);
  const sc = buildScene(r), dur = r.duration_s, tr = r.echo.truth;
  const set = E.set;
  if (!set.depth) set.depth = Math.round(Math.min(18, Math.max(5, 4.2 * Math.cbrt(r.echo.bw))) * 2) / 2;
  if (!set.focus) set.focus = set.depth * 0.5;
  if (!set.freq) set.freq = set.harmonic ? (r.echo.bw < 10 ? 3.5 : r.echo.bw < 25 ? 1.7 : 1.5) : (r.echo.bw < 10 ? 7 : r.echo.bw < 25 ? 5 : 3.5);
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
        <label class="inline">Doppler <select id="e-dop">${[['off', 'aucun'], ['color', 'couleur'], ['pw', 'pulsé (PW)'], ['cw', 'continu (CW)'], ['color+pw', 'couleur + PW'], ['color+cw', 'couleur + CW']].map(([k, l]) => `<option value="${k}" ${E.dop === k ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
        <label class="inline"><input type="checkbox" id="e-flip" ${set.flip ? 'checked' : ''}> Inverser gauche-droite</label>
        <label class="inline"><input type="checkbox" id="e-harm" ${set.harmonic ? 'checked' : ''}> Harmonique</label>
      </div>
      <canvas id="e-img" width="640" height="560" class="ec-img"></canvas>
      <canvas id="e-mmc" width="640" height="300" class="ec-img" ${E.mmode ? '' : 'hidden'}></canvas>
      <canvas id="e-spec" width="640" height="300" class="ec-img" ${/pw|cw/.test(E.dop) ? '' : 'hidden'}></canvas>
      <div class="note" id="e-dopinfo"></div>
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
      ${ui.card('Doppler', `
        ${slider('e-cs', 'Échelle couleur (Nyquist)', 10, 150, 1, set.cScale, ' cm/s')}
        ${slider('e-wf', 'Filtre de paroi', 0, 0.3, 0.01, set.wallFilter, '')}
        ${slider('e-pws', 'Échelle PW (Nyquist)', 20, 300, 1, set.pwScale, ' cm/s')}
        ${slider('e-cws', 'Échelle CW', 100, 800, 10, set.cwScale, ' cm/s')}
        ${slider('e-bx', 'Boîte couleur : centre', -40, 40, 1, E.box.c, '°')}
        ${slider('e-bw', 'Boîte couleur : largeur', 10, 80, 1, E.box.w, '°')}
        ${slider('e-br0', 'Boîte couleur : haut', 0.5, 15, 0.1, E.box.r0, ' cm')}
        ${slider('e-br1', 'Boîte couleur : bas', 1, 20, 0.1, E.box.r1, ' cm')}
        <div class="note">PW/CW : clic sur l'image (pendant la lecture, ou Maj + clic) pour placer la ligne et la porte. La limite de Nyquist dépend de la profondeur (fréquence de répétition maximale) : au-delà, repliement spectral.</div>`)}
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
    const env = eng.env;
    if (!eng.prev || eng.prev.length !== env.length) eng.prev = new Float32Array(env);
    const pk = E.playing ? set.persist : 0;
    for (let i = 0; i < env.length; i++) { env[i] = pk * eng.prev[i] + (1 - pk) * env[i]; eng.prev[i] = env[i]; }
    const d = img.data;
    let col = null;
    if (/color/.test(E.dop)) {
      const bx = { th0: (E.box.c - E.box.w / 2) * D2R, th1: (E.box.c + E.box.w / 2) * D2R, r0: E.box.r0, r1: Math.min(set.depth, E.box.r1) };
      col = colorDoppler(sc, G, dopplerFrame(r, E.t), r.echo.geometry, f, set, E.t, bx, eng);
      col.bx = bx;
    }
    for (let p = 0; p < W * IH; p++) {
      const i = eng.lut[p];
      let g = 0;
      if (i >= 0) {
        const fx = eng.lutF[2 * p], fy = eng.lutF[2 * p + 1];
        g = (env[i] * (1 - fy) + env[i + 1] * fy) * (1 - fx) + (env[i + NS] * (1 - fy) + env[i + NS + 1] * fy) * fx;
      }
      const c = Math.round(255 * g);
      d[4 * p] = c; d[4 * p + 1] = c; d[4 * p + 2] = Math.min(255, c + 4); d[4 * p + 3] = 255;
      if (col && i >= 0 && g < 0.62) {
        const li = Math.floor(i / NS) + eng.lutF[2 * p], si = (i % NS) + eng.lutF[2 * p + 1];
        const a = Math.round((li - col.l0) / col.LS), k = Math.round((si - col.s0) / col.SS);
        if (a >= 0 && a < col.nl && k >= 0 && k < col.ns) {
          const v = col.vel[a * col.ns + k];
          if (!Number.isNaN(v)) { const rgb = colorOf(v, col.vr[a * col.ns + k]); d[4 * p] = rgb[0]; d[4 * p + 1] = rgb[1]; d[4 * p + 2] = rgb[2]; }
        }
      }
    }
    E.lastCol = col;
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
    ctx.fillText(`${set.harmonic ? `H ${fmt(set.freq, 1)}/${fmt(2 * set.freq, 1)}` : fmt(set.freq, 1)} MHz  ·  prof. ${fmt(set.depth, 1)} cm  ·  gain ${set.gain} dB  ·  GD ${set.dr} dB`, 8, 16);
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
    const P2 = (th, rr) => [eng.cx + Math.sin(th) * rr * eng.scale, 4 + Math.cos(th) * rr * eng.scale];
    if (E.lastCol) {
      const b = E.lastCol.bx;
      ctx.strokeStyle = 'rgba(255,255,255,.55)'; ctx.lineWidth = 1; ctx.beginPath();
      let q = P2(b.th0, b.r0); ctx.moveTo(q[0], q[1]);
      for (let a = b.th0; a <= b.th1 + 1e-6; a += (b.th1 - b.th0) / 16) { q = P2(a, b.r0); ctx.lineTo(q[0], q[1]); }
      for (let a = b.th1; a >= b.th0 - 1e-6; a -= (b.th1 - b.th0) / 16) { q = P2(a, b.r1); ctx.lineTo(q[0], q[1]); }
      ctx.closePath(); ctx.stroke();
      // barre de couleurs et limite de Nyquist
      for (let y = 0; y < 100; y++) { const v = 1 - y / 50; const c = colorOf(v, 0); ctx.fillStyle = `rgb(${c[0]},${c[1]},${c[2]})`; ctx.fillRect(10, 40 + y, 10, 1); }
      ctx.fillStyle = '#ddd'; ctx.font = '11px system-ui';
      ctx.fillText(`+${fmt(E.lastCol.vn, 0)}`, 24, 46); ctx.fillText(`−${fmt(E.lastCol.vn, 0)} cm/s`, 24, 140);
    }
    if (/pw|cw/.test(E.dop)) {
      const g = E.gate, a = P2(g.theta, 0), b = P2(g.theta, set.depth);
      ctx.strokeStyle = 'rgba(160,255,160,.75)'; ctx.setLineDash([3, 4]); ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); ctx.stroke(); ctx.setLineDash([]);
      if (/pw/.test(E.dop)) {
        const nx = Math.cos(g.theta) * 6, ny = -Math.sin(g.theta) * 6;
        for (const rr of [g.r - set.gateLen / 2, g.r + set.gateLen / 2]) { const c = P2(g.theta, rr); ctx.beginPath(); ctx.moveTo(c[0] - nx, c[1] - ny); ctx.lineTo(c[0] + nx, c[1] + ny); ctx.stroke(); }
      } else { const c = P2(g.theta, g.r); ctx.beginPath(); ctx.arc(c[0], c[1], 4, 0, 7); ctx.stroke(); }
    }
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

  // ---------------------------------------------- spectre Doppler (PW / CW)
  const sp = $('#e-spec'), sctx = sp.getContext('2d');
  const SW = sp.width, SH = sp.height - 50, NB = SH;
  const spBuf = E.spBuf && E.spBuf.width === SW ? E.spBuf : (E.spBuf = sctx.createImageData(SW, SH));
  E.spT = E.spT || [];
  function drawS(advanceTo) {
    const mode = /pw/.test(E.dop) ? 'pw' : 'cw', sweep = 3, colDt = sweep / SW;
    const vmax = mode === 'cw' ? set.cwScale : Math.min(nyquistMax(set, E.gate.r), set.pwScale);
    E.spScale = vmax;
    if (E.sLast === undefined || advanceTo < E.sLast || advanceTo - E.sLast > sweep) E.sLast = advanceTo - colDt;
    let n = 0;
    while (E.sLast + colDt <= advanceTo && n < 30) {
      E.sLast += colDt; n++;
      const col = Math.floor(((E.sLast % sweep) + sweep) % sweep / colDt) % SW;
      const { hist } = spectrumColumn(sc, r, set, pose(), E.gate, E.sLast, mode, NB, vmax);
      const mx = mode === 'pw' ? 30 : 60, dd = spBuf.data;
      for (let y = 0; y < SH; y++) {
        const hv = hist[NB - 1 - y];
        const g = Math.min(255, 255 * Math.log1p(hv * 3) / Math.log1p(mx * 3) + 10 * Math.random());
        const p = 4 * (y * SW + col); dd[p] = g; dd[p + 1] = g; dd[p + 2] = g; dd[p + 3] = 255;
      }
      E.spT[col] = E.sLast;
    }
    sctx.fillStyle = '#000'; sctx.fillRect(0, 0, SW, sp.height);
    sctx.putImageData(spBuf, 0, 0);
    const cur = Math.floor(((E.sLast % sweep) + sweep) % sweep / colDt);
    sctx.fillStyle = '#e0465a'; sctx.fillRect(cur, 0, 2, SH);
    // ligne de base et graduations (m/s ; positif = vers la sonde)
    sctx.strokeStyle = '#6aa'; sctx.fillStyle = '#9fd3c7'; sctx.font = '11px system-ui';
    sctx.beginPath(); sctx.moveTo(0, SH / 2); sctx.lineTo(SW, SH / 2); sctx.stroke();
    const step = vmax > 300 ? 100 : vmax > 120 ? 50 : 20;
    for (let v = -Math.floor(vmax / step) * step; v <= vmax; v += step) {
      const y = SH / 2 - v / vmax * SH / 2; sctx.fillRect(SW - 10, y, 10, 1); sctx.fillText(fmt(v / 100, step >= 100 ? 0 : 1), SW - 40, y + 4);
    }
    sctx.fillText(`${mode === 'pw' ? 'PW' : 'CW'} · ${fmt(dopFreq(set), 1)} MHz · échelle ±${fmt(vmax / 100, 2)} m/s${mode === 'pw' ? ` (Nyquist max à ${fmt(E.gate.r, 1)} cm : ${fmt(nyquistMax(set, E.gate.r) / 100, 2)} m/s)` : ''} · m/s`, 8, 14);
    // ECG
    sctx.strokeStyle = '#3ad16b'; sctx.beginPath();
    for (let px = 0; px < SW; px++) {
      const tt = E.spT[px]; if (tt === undefined) continue;
      const val = r.ecg[Math.round(tt * r.ecg_fs)] || 0, y = SH + 30 - val * 14;
      if (px === 0 || E.spT[px - 1] === undefined || Math.abs(E.spT[px - 1] - tt) > 0.1) sctx.moveTo(px, y); else sctx.lineTo(px, y);
    }
    sctx.stroke();
    for (const m of E.meas) if (m.where === 's') {
      const y = SH / 2 - m.v / vmax * SH / 2; sctx.strokeStyle = '#ffe26a'; sctx.beginPath(); sctx.moveTo(m.x - 12, y); sctx.lineTo(m.x + 12, y); sctx.stroke();
      sctx.fillStyle = '#ffe26a'; sctx.fillText(`${m.lab ? labOf(m.lab) + ' ' : ''}${fmt(m.v / 100, 2)} m/s · ${fmt(4 * (m.v / 100) ** 2, 0)} mmHg`, m.x + 14, y - 4);
    }
  }
  function dopInfo() {
    if (!/pw|cw/.test(E.dop)) { $('#e-dopinfo').textContent = ''; return; }
    const G = geomNow(), Dd = dopplerFrame(r, E.t), f = probeFrame(sc, pose());
    if (set.flip) f.L = mul(f.L, -1);
    const c = Math.cos(E.gate.theta), s2 = Math.sin(E.gate.theta);
    const u = [f.b[0] * c + f.L[0] * s2, f.b[1] * c + f.L[1] * s2, f.b[2] * c + f.L[2] * s2];
    const q = dopplerAt(sc, G, Dd, r.echo.geometry, add(f.P, mul(u, E.gate.r)), u, E.t);
    $('#e-dopinfo').textContent = q && q.speed > 5
      ? `À la porte, maintenant : vitesse du sang ${fmt(q.speed / 100, 2)} m/s, angle faisceau/flux ${fmt(Math.acos(Math.min(1, Math.abs(q.v) / q.speed)) / D2R, 0)}° → vitesse mesurée ${fmt(q.v / 100, 2)} m/s (cos θ).`
      : 'Porte hors d\'un flux notable à cet instant.';
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
    const dt = r.echo.doppler.truth;
    const dTruth = { dE: dt.e, dA: dt.a, dAo: dt.ao_peak, dMR: dt.mr_peak, dTR: dt.tr_peak };
    const rows = E.meas.filter((m) => m.lab).map((m) => m.where === 's'
      ? `<tr><td>${labOf(m.lab)}</td><td>${fmt(Math.abs(m.v) / 100, 2)} m/s (${fmt(4 * (m.v / 100) ** 2, 0)} mmHg)</td>${E.showTruth ? `<td>${dTruth[m.lab] ? fmt(dTruth[m.lab] / 100, 2) + ' m/s' : '—'}</td>` : ''}</tr>`
      : `<tr><td>${labOf(m.lab)}</td><td>${fmt(m.cm, 2)} cm</td>${E.showTruth ? `<td>${truthOf(m.lab)}</td>` : ''}</tr>`);
    const get = (k) => { const x = E.meas.filter((m) => m.lab === k); return x.length ? x.reduce((a, m) => a + (m.where === 's' ? Math.abs(m.v) : m.cm), 0) / x.length : null; };
    const dv = [];
    const lvd = get('lvidd'), lvs = get('lvids'), la = get('la'), ao = get('ao');
    if (lvd && lvs) dv.push(`FR = ${fmt(100 * (1 - lvs / lvd), 0)} %${E.showTruth ? ` (modèle ${fmt(100 * tr.fs, 0)} %)` : ''}`);
    if (lvd) dv.push(`LVIDdN = ${fmt(lvd / Math.pow(r.echo.bw, 0.294), 2)}${E.showTruth ? ` (modèle ${fmt(tr.lvidd_n, 2)})` : ''}`);
    if (la && ao) dv.push(`OG/Ao = ${fmt(la / ao, 2)}${E.showTruth ? ` (modèle ${fmt(tr.la_ao, 2)})` : ''}`);
    const dE = get('dE'), dA = get('dA');
    if (dE && dA) dv.push(`E/A = ${fmt(dE / dA, 2)}${E.showTruth && dt.e_a ? ` (modèle ${fmt(dt.e_a, 2)})` : ''}`);
    $('#e-mlist').innerHTML = (rows.length ? `<table class="ec-tab"><tr><th>Mesure</th><th>Vous</th>${E.showTruth ? '<th>Modèle</th>' : ''}</tr>${rows.join('')}</table>` : '')
      + (dv.length ? `<div class="note">${dv.join(' · ')}</div>` : '')
      + (E.showTruth ? `<div class="note">Modèle (coupe parfaite, moyenne de ${tr.n_beats} battements) : LVIDd ${fmt(tr.lvidd, 2)} cm, LVIDs ${fmt(tr.lvids, 2)} cm, paroi ${fmt(tr.wall_d, 2)}/${fmt(tr.wall_s, 2)} cm, Ao ${fmt(tr.ao, 2)} cm, OG ${fmt(tr.la_d, 2)} cm (fermeture aortique), volume de l'OG ${fmt(tr.la_min, 1)}–${fmt(tr.la_max, 1)} mL (${fmt(tr.la_max / r.echo.bw, 2)} mL/kg), FE ${fmt(100 * tr.ef, 0)} %. Doppler : E ${fmt(dt.e / 100, 2)} m/s, A ${fmt(dt.a / 100, 2)} m/s${dt.e_a ? `, E/A ${fmt(dt.e_a, 2)}` : ' (pas d\'onde A)'}, Vmax aortique ${fmt(dt.ao_peak / 100, 2)} m/s (ITV ${fmt(dt.ao_vti, 1)} cm), pulmonaire ${fmt(dt.pu_peak / 100, 2)} m/s${dt.pu_at ? ` (TA ${fmt(dt.pu_at, 0)} ms, TA/TE ${fmt(dt.pu_at_et, 2)})` : ''}${tr.mpa_ao ? `, tronc pulmonaire ${fmt(tr.mpa, 2)} cm (AP/Ao ${fmt(tr.mpa_ao, 2)})` : ''}${dt.mr_peak ? `, IM ${fmt(dt.mr_peak / 100, 2)} m/s (EROA ${fmt(dt.eroa_mr, 2)} cm²)` : ''}${dt.tr_peak ? `, IT ${fmt(dt.tr_peak / 100, 2)} m/s → PAPs estimée ${fmt(dt.pasp_est, 0)} mmHg (modèle ${fmt(dt.pasp_true, 0)})` : ''}.</div>` : '');
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
      if (/pw|cw/.test(E.dop)) { drawS(E.t); dopInfo(); }
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
  $('#e-dop').onchange = (ev) => { E.dop = ev.target.value; sp.hidden = !/pw|cw/.test(E.dop); E.sLast = undefined; E.spT = []; dirty = true; };
  bindSet('e-cs', 'cScale', ' cm/s', 0); bindSet('e-wf', 'wallFilter', '', 2); bindSet('e-pws', 'pwScale', ' cm/s', 0); bindSet('e-cws', 'cwScale', ' cm/s', 0);
  for (const [id, k, d, u] of [['e-bx', 'c', 0, '°'], ['e-bw', 'w', 0, '°'], ['e-br0', 'r0', 1, ' cm'], ['e-br1', 'r1', 1, ' cm']]) {
    $('#' + id).oninput = (ev) => { E.box[k] = +ev.target.value; $('#' + id + '-v').textContent = fmt(E.box[k], d) + u; dirty = true; };
  }
  $('#e-flip').onchange = (ev) => { set.flip = ev.target.checked; dirty = true; };
  $('#e-harm').onchange = (ev) => { set.harmonic = ev.target.checked; dirty = true; E.mLast = undefined; };
  $('#e-mclear').onclick = () => { E.meas = []; renderMeas(); dirty = true; };
  $('#e-truth').onclick = () => { E.showTruth = !E.showTruth; $('#e-truth').textContent = (E.showTruth ? 'Masquer' : 'Comparer au') + ' modèle'; renderMeas(); };

  // calipers et ligne TM
  const toCm = (ev) => {
    const b = cv.getBoundingClientRect(), px = (ev.clientX - b.left) * (cv.width / b.width), py = (ev.clientY - b.top) * (cv.height / b.height);
    return [(px - eng.cx) / eng.scale, (py - 4) / eng.scale];
  };
  cv.onpointerdown = (ev) => {
    const p = toCm(ev);
    if (/pw|cw/.test(E.dop) && (ev.shiftKey || E.playing)) {
      E.gate = { theta: Math.atan2(p[0], p[1]), r: Math.hypot(p[0], p[1]) }; E.sLast = undefined; E.spT = []; dirty = true; return;
    }
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
  sp.onpointerdown = (ev) => {
    if (E.playing) return;
    const b = sp.getBoundingClientRect(), x = (ev.clientX - b.left) * (sp.width / b.width), y = (ev.clientY - b.top) * (sp.height / b.height);
    const v = (SH / 2 - y) / (SH / 2) * E.spScale;
    E.meas.push({ where: 's', x, v, lab: $('#e-mlab').value }); renderMeas(); dirty = true;
  };

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

const MEAS = [['dE', 'E mitrale (Doppler)'], ['dA', 'A mitrale (Doppler)'], ['dAo', 'Vmax aortique (Doppler)'], ['dMR', 'Vmax IM (Doppler)'], ['dTR', 'Vmax IT (Doppler)'],
  ['lvidd', 'LVIDd (VG télédiastole)'], ['lvids', 'LVIDs (VG télésystole)'], ['ivsd', 'SIVd'], ['lvwd', 'PPVGd'],
  ['ivss', 'SIVs'], ['lvws', 'PPVGs'], ['la', 'OG (petit axe, fermeture Ao)'], ['ao', 'Ao (petit axe)'], ['', 'Autre (sans étiquette)']];
const labOf = (k) => (MEAS.find((m) => m[0] === k) || ['', ''])[1].split(' (')[0];

// Accès aux fonctions internes pour les tests et le diagnostic (aucun effet sur l'interface).
// Statistiques des niveaux de gris par tissu (0–255) sur une image calculée hors affichage.
function grayStats(r, set, viewIdx, t) {
  const sc = buildScene(r), G = frameGeometry(r, sc, t);
  const pose = solveView(sc, G, views(sc, G)[viewIdx], t), f = probeFrame(sc, pose);
  const NL = set.lines, NS = Math.round(set.depth / 0.025), half = set.sector / 2 * D2R, dth = 2 * half / (NL - 1);
  const thetas = Array.from({ length: NL }, (_, i) => -half + i * dth);
  const re = new Float32Array(NL * NS), im = new Float32Array(NL * NS), a = new Float32Array(NL * NS), b = new Float32Array(NL * NS), env = new Float32Array(NL * NS), ids = new Uint8Array(NL * NS);
  const lam = acquire(sc, G, f, set, t, thetas, re, im, NS, 1, ids);
  psf(set, NL, NS, re, im, a, b, lam, dth);
  toGray(set, re, im, env, NL, NS);
  const out = {};
  for (const [name, id] of Object.entries(T)) {
    const v = []; for (let i = 0; i < env.length; i++) if (ids[i] === id && (i % NS) * set.depth / NS > 0.3) v.push(255 * env[i]);
    if (v.length < 50) continue;
    v.sort((x, y) => x - y);
    const q = (p) => Math.round(v[Math.floor(p * (v.length - 1))]);
    out[name] = { n: v.length, p10: q(0.1), med: q(0.5), p90: q(0.9) };
  }
  return out;
}
// Position (angle dans le secteur, profondeur) d'un point du repère cardiaque dans l'image.
function projectHeart(r, pose, set, t, hk) {
  const sc = buildScene(r), G = frameGeometry(r, sc, t), f = probeFrame(sc, pose);
  if (set.flip) f.L = mul(f.L, -1);
  const h = typeof hk === 'function' ? hk(G) : hk;
  const q = sub(fromHeart(sc, h), f.P);
  return { theta: Math.atan2(dot(q, f.L), dot(q, f.b)), r: Math.hypot(...q) };
}
// Diagnostic : régions et vitesses le long d'une ligne.
function probeLine(r, pose, set, t, theta) {
  const sc = buildScene(r), G = frameGeometry(r, sc, t), f = probeFrame(sc, pose), Dd = dopplerFrame(r, t);
  if (set.flip) f.L = mul(f.L, -1);
  const u = norm(add(mul(f.b, Math.cos(theta)), mul(f.L, Math.sin(theta))));
  const out = [];
  for (let rr = 0.2; rr < set.depth; rr += 0.25) {
    const q = dopplerAt(sc, G, Dd, r.echo.geometry, add(f.P, mul(u, rr)), u, t);
    out.push(q ? `${rr.toFixed(2)}:${Object.keys(RG).find((k) => RG[k] === REGION)}:${q.v.toFixed(0)}` : `${rr.toFixed(2)}:-`);
  }
  return { Dd, line: out.join(' ') };
}
export const _internals = { buildScene, frameGeometry, classify, probeFrame, solveView, views, toHeart, T, grayStats, projectHeart, probeLine, flowAt, dopplerFrame, FLOW, RG };
