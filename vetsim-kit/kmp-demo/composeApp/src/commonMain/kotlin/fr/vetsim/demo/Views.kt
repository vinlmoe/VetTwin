package fr.vetsim.demo

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.sin

private val Ink = Color(0xFF1B2A35)
private val Muted = Color(0xFF5B6B77)
private val DiseaseRed = Color(0xFFC0392B)
private val DrugBlue = Color(0xFF2563A8)
private val EdgeGray = Color(0xFF8A99A6)

// ------------------------------------------------------------------------------ respiration

/** Courbes respiratoires du modèle (même fenêtre que l'ECG) et leurs couplages visibles. */
@Composable
fun RespirationView(r: DemoResult) {
    val rs = r.respiration
    val seconds = (rs.volume.size / rs.fs).toInt()
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "FR ${"%.0f".fmt(rs.rrPerMin)} /min · volume courant alvéolaire ${"%.0f".fmt(rs.vtAlvMl)} mL " +
                "(VA/FR ; l'espace mort n'est pas modélisé) · inspiration sur 40 % du cycle",
            color = Muted, fontSize = 13.sp,
        )
        TracePanel("Volume pulmonaire au-dessus de la fin d'expiration (mL), $seconds s", rs.volume, Color(0xFF0B5E7A), unit = "mL")
        TracePanel("Débit respiratoire (mL/s) : > 0 inspiration, < 0 expiration", rs.flow, Color(0xFF5B8C2A), unit = "mL/s", zeroLine = true)
        TracePanel("Fréquence cardiaque instantanée (/min) : arythmie sinusale respiratoire", rs.hr, Color(0xFF0B7A55), unit = "/min")
        TracePanel(
            "Pressions artérielles instantanées (mmHg) : systolique, moyenne, diastolique",
            listOf(rs.sap to DiseaseRed, rs.map to Color(0xFF7A3E9D), rs.dap to DrugBlue),
            unit = "mmHg", height = 160,
            legend = listOf("PAS" to DiseaseRed, "PAM" to Color(0xFF7A3E9D), "PAD" to DrugBlue),
        )
        Text(
            "PAS et PAD = PAM instantanée du modèle autonome ± écart du battement simulé " +
                "(PAS ${"%.0f".fmt(rs.sap.average())}, PAD ${"%.0f".fmt(rs.dap.average())} mmHg en moyenne) : " +
                "la pression pulsée est supposée constante, le modèle ne simule pas les battements un par un.",
            color = Muted, fontSize = 12.sp,
        )
        Text(
            "Mêmes instants que l'ECG : à l'inspiration, le tonus vagal central est inhibé (porte respiratoire), " +
                "la FC monte ; la PA varie avec le volume pulmonaire et, avec retard, par le baroréflexe.",
            color = Muted, fontSize = 12.sp,
        )
    }
}

@Composable
private fun TracePanel(title: String, v: FloatArray, color: Color, unit: String, zeroLine: Boolean = false) =
    TracePanel(title, listOf(v to color), unit, zeroLine = zeroLine)

@Composable
private fun TracePanel(
    title: String,
    series: List<Pair<FloatArray, Color>>,
    unit: String,
    zeroLine: Boolean = false,
    height: Int = 110,
    legend: List<Pair<String, Color>> = emptyList(),
) {
    val measurer = rememberTextMeasurer()
    Column(Modifier.fillMaxWidth().card().padding(12.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(title, fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
            legend.forEach { (l, c) ->
                Box(Modifier.width(16.dp).height(3.dp).background(c))
                Text(l, color = Muted, fontSize = 12.sp)
            }
        }
        Canvas(Modifier.fillMaxWidth().height(height.dp)) {
            val all = series.flatMap { it.first.asIterable() }
            if (all.isEmpty()) return@Canvas
            val lo = all.min()
            val hi = all.max()
            val span = (hi - lo).takeIf { it > 1e-6f } ?: 1f
            val left = 44f
            val w = size.width - left
            fun y(x: Float) = 6f + (1f - (x - lo) / span) * (size.height - 12f)
            drawYAxis(measurer, lo, hi, left, size.width, size.height, ::y)
            if (zeroLine && lo < 0f && hi > 0f) drawLine(EdgeGray, Offset(left, y(0f)), Offset(size.width, y(0f)))
            for ((v, color) in series) {
                val path = Path()
                v.forEachIndexed { i, x ->
                    val px = left + i.toFloat() / (v.size - 1).coerceAtLeast(1) * w
                    if (i == 0) path.moveTo(px, y(x)) else path.lineTo(px, y(x))
                }
                drawPath(path, color, style = Stroke(width = 2f))
            }
        }
    }
}

// ------------------------------------------------------------------------------ schéma

private data class Node(val id: String, val col: Int, val row: Int, val kind: Kind)
private enum class Kind { Block, Source, Output }
private data class Edge(val from: String, val to: String, val label: String, val bidir: Boolean = false, val bend: Float = 0f)

/** Disposition : 5 colonnes × 4 lignes. */
private val NODES = listOf(
    Node("patient", 0, 0, Kind.Block), Node("volume", 1, 0, Kind.Block), Node("autonomic", 2, 0, Kind.Block),
    Node("disease", 3, 0, Kind.Source), Node("drugs", 4, 0, Kind.Source),
    Node("systemic_circulation", 0, 1, Kind.Block), Node("right_heart", 1, 1, Kind.Block), Node("valves", 2, 1, Kind.Block),
    Node("left_heart", 3, 1, Kind.Block), Node("pulmonary_circulation", 4, 1, Kind.Block),
    Node("kidney", 0, 2, Kind.Block), Node("metabolism", 1, 2, Kind.Block), Node("oxygen_transport", 2, 2, Kind.Block),
    Node("acid_base", 3, 2, Kind.Block), Node("ventilation", 4, 2, Kind.Block),
    Node("liver", 0, 3, Kind.Block), Node("ecg", 1, 3, Kind.Block), Node("pcg", 2, 3, Kind.Output),
    Node("gases", 3, 3, Kind.Output), Node("respiration", 4, 3, Kind.Output),
)

/** Couplages codés dans le moteur (hemo.rs, engine.rs, gas.rs, autonomic.rs, signals.rs). */
private val EDGES = listOf(
    Edge("patient", "volume", "85 mL/kg × bvf"),
    Edge("volume", "systemic_circulation", "volume veineux"),
    Edge("autonomic", "systemic_circulation", "PAM ⇄ r_sys, recrutement", bidir = true),
    Edge("autonomic", "right_heart", "inotropie"),
    Edge("autonomic", "left_heart", "inotropie, FC"),
    Edge("systemic_circulation", "right_heart", "retour veineux"),
    Edge("right_heart", "valves", "tricuspide, pulmonaire", bidir = true),
    Edge("valves", "left_heart", "mitrale, aortique", bidir = true),
    Edge("right_heart", "pulmonary_circulation", "débit pulmonaire", bend = -0.22f),
    Edge("pulmonary_circulation", "left_heart", "remplissage (POG)"),
    Edge("left_heart", "systemic_circulation", "éjection aortique", bend = 0.16f),
    Edge("systemic_circulation", "kidney", "PAM, DC → perfusion"),
    Edge("systemic_circulation", "metabolism", "DC → DO₂"),
    Edge("oxygen_transport", "metabolism", "CaO₂"),
    Edge("metabolism", "acid_base", "lactate", bend = 0.25f),
    Edge("acid_base", "ventilation", "pH ⇄ PaCO₂", bidir = true),
    Edge("ventilation", "oxygen_transport", "PaO₂", bend = -0.25f),
    Edge("acid_base", "oxygen_transport", "pH (Bohr)"),
    Edge("pulmonary_circulation", "ventilation", "POG → œdème (A-a)"),
    Edge("ventilation", "autonomic", "respiration → RSA", bend = 0.12f),
    Edge("autonomic", "ecg", "instants R", bend = -0.08f),
    Edge("acid_base", "ecg", "K⁺", bend = 0.1f),
    Edge("valves", "pcg", "débits valvulaires", bend = 0.05f),
    Edge("acid_base", "gases", "pH, HCO₃⁻, BE"),
    Edge("oxygen_transport", "gases", "SaO₂, PvO₂"),
    Edge("ventilation", "respiration", "FR, volume courant"),
)

/**
 * Schéma du modèle : blocs, couplages, et blocs touchés par la maladie et les médicaments
 * choisis (paramètres différents de ceux du chien sain de même poids). Cliquer un bloc affiche
 * ses paramètres et ses couplages.
 */
@Composable
fun ModelView(catalog: Catalog, r: DemoResult?, initialSelection: String? = null) {
    var selected by remember { mutableStateOf(initialSelection) }
    val changes = r?.changes ?: emptyList()
    val disease = catalog.diseases.firstOrNull { it.id == r?.diseaseId }
    val drugs = catalog.drugs.filter { it.id in (r?.drugIds ?: emptyList()) }
    val diseaseBlocks = disease?.effects?.flatMap { it.blocks }?.toSet() ?: emptySet()
    val drugBlocks = drugs.flatMap { d -> d.effects.flatMap { it.blocks } }.toSet()
    val changedBlocks = changes.flatMap { it.blocks }.groupingBy { it }.eachCount()
    val label = { id: String ->
        when (id) {
            "disease" -> disease?.let { "Maladie : ${it.name.substringAfter('(').substringBefore(')').takeIf { s -> s != it.name } ?: it.name}" } ?: "Maladie (module JSON)"
            "drugs" -> if (drugs.isEmpty()) "Médicaments (PK/PD)" else "Médicaments : " + drugs.joinToString { it.name }
            "pcg" -> "PCG / auscultation"
            "gases" -> "Gaz du sang, biochimie"
            "respiration" -> "Courbes respiratoires"
            else -> catalog.blocks.firstOrNull { it.id == id }?.label ?: id
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Swatch(Color(0xFFFDE7E4), DiseaseRed); Text("modifié par la maladie", color = Muted, fontSize = 12.sp)
            Swatch(Color(0xFFE3EDF9), DrugBlue); Text("modifié par un médicament", color = Muted, fontSize = 12.sp)
            Swatch(Color(0xFFF1F4F6), EdgeGray); Text("sortie du modèle", color = Muted, fontSize = 12.sp)
            Text("Cliquer un bloc pour ses paramètres.", color = Muted, fontSize = 12.sp)
        }
        val measurer = rememberTextMeasurer()
        BoxWithConstraints(Modifier.fillMaxWidth().height(560.dp).card()) {
            val wPx = constraints.maxWidth.toFloat()
            val hPx = constraints.maxHeight.toFloat()
            val nodeW = wPx / 5f - 56f
            val nodeH = hPx / 4f - 44f
            fun center(n: Node) = Offset((n.col + 0.5f) * wPx / 5f, (n.row + 0.5f) * hPx / 4f)
            val byId = NODES.associateBy { it.id }
            Canvas(Modifier.fillMaxSize()) {
                val box = Size(nodeW, nodeH)
                val curves = mutableListOf<Curve>()
                for (e in EDGES) {
                    val a = center(byId.getValue(e.from))
                    val b = center(byId.getValue(e.to))
                    drawEdge(a, b, box, e.bend, e.bidir, EdgeGray, null)
                    curves += edgeCurve(a, b, box, e.bend)
                }
                // Liens dynamiques : module → blocs où il agit.
                diseaseBlocks.forEach { b ->
                    byId[b]?.let {
                        drawEdge(center(byId.getValue("disease")), center(it), box, 0.08f, false, DiseaseRed, dashed)
                        curves += edgeCurve(center(byId.getValue("disease")), center(it), box, 0.08f)
                    }
                }
                drugBlocks.forEach { b ->
                    byId[b]?.let {
                        drawEdge(center(byId.getValue("drugs")), center(it), box, -0.08f, false, DrugBlue, dashed)
                        curves += edgeCurve(center(byId.getValue("drugs")), center(it), box, -0.08f)
                    }
                }
                // Libellés des couplages, placés sans chevauchement après tous les liens.
                val style = TextStyle(fontSize = 10.sp, color = Muted)
                val layouts = EDGES.map { measurer.measure(it.label, style) }
                val boxes = NODES.map { n ->
                    val c = center(n)
                    Rect(c.x - nodeW / 2 - 2f, c.y - nodeH / 2 - 2f, c.x + nodeW / 2 + 2f, c.y + nodeH / 2 + 2f)
                }
                val spots = placeEdgeLabels(
                    EDGES.indices.map { LabelItem(curves[it], layouts[it].size.width + 8f, layouts[it].size.height + 1f, it) },
                    curves, boxes, size.width, size.height,
                )
                for ((i, p) in spots) {
                    val l = layouts[i]
                    val tl = Offset(p.x - l.size.width / 2f, p.y - l.size.height / 2f)
                    drawRect(Color.White.copy(alpha = 0.9f), tl, Size(l.size.width.toFloat(), l.size.height.toFloat()))
                    drawText(l, topLeft = tl)
                }
            }
            for (n in NODES) {
                val c = center(n)
                val (fill, stroke) = when {
                    n.id == "disease" && disease != null -> Color(0xFFFDE7E4) to DiseaseRed
                    n.id == "drugs" && drugs.isNotEmpty() -> Color(0xFFE3EDF9) to DrugBlue
                    n.kind == Kind.Output || n.kind == Kind.Source -> Color(0xFFF1F4F6) to EdgeGray
                    n.id in diseaseBlocks && changedBlocks.containsKey(n.id) -> Color(0xFFFDE7E4) to DiseaseRed
                    n.id in drugBlocks && changedBlocks.containsKey(n.id) -> Color(0xFFE3EDF9) to DrugBlue
                    else -> Color.White to Color(0xFFB8C4CE)
                }
                val density = androidx.compose.ui.platform.LocalDensity.current
                with(density) {
                    Box(
                        Modifier
                            .offset((c.x - nodeW / 2f).toDp(), (c.y - nodeH / 2f).toDp())
                            .size(nodeW.toDp(), nodeH.toDp())
                            .background(fill, RoundedCornerShape(10.dp))
                            .border(if (selected == n.id) 3.dp else 1.5.dp, if (selected == n.id) Ink else stroke, RoundedCornerShape(10.dp))
                            .clickable(enabled = n.kind == Kind.Block) { selected = if (selected == n.id) null else n.id }
                            .padding(6.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        val count = changedBlocks[n.id]
                        Text(
                            label(n.id) + if (count != null && n.kind == Kind.Block) "\n$count paramètre${if (count > 1) "s" else ""} modifié${if (count > 1) "s" else ""}" else "",
                            color = Ink, fontSize = 11.sp, lineHeight = 14.sp, textAlign = TextAlign.Center,
                            fontWeight = if (count != null) FontWeight.SemiBold else FontWeight.Normal,
                        )
                    }
                }
            }
        }
        selected?.let { id -> BlockDetail(catalog, id, label(id), changes) }
        ChangesTable(catalog, changes)
    }
}

private val dashed = PathEffect.dashPathEffect(floatArrayOf(10f, 8f))

@Composable
private fun Swatch(fill: Color, stroke: Color) {
    Box(Modifier.size(16.dp).background(fill, RoundedCornerShape(4.dp)).border(1.5.dp, stroke, RoundedCornerShape(4.dp)))
}

@Composable
private fun BlockDetail(catalog: Catalog, id: String, label: String, changes: List<ParamChange>) {
    val block = catalog.blocks.firstOrNull { it.id == id } ?: return
    val byParam = changes.associateBy { it.param }
    Column(Modifier.fillMaxWidth().card().padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 15.sp)
        val couplings = EDGES.filter { it.from == id || it.to == id }.joinToString(" · ") {
            val other = if (it.from == id) it.to else it.from
            val arrow = if (it.bidir) "⇄" else if (it.from == id) "→" else "←"
            "$arrow ${catalog.blocks.firstOrNull { b -> b.id == other }?.label ?: other} (${it.label})"
        }
        Text("Couplages : $couplings", color = Muted, fontSize = 12.sp)
        Text("Paramètres de ce bloc :", color = Ink, fontSize = 13.sp)
        block.params.forEach { p ->
            val c = byParam[p]
            val txt = if (c == null) "• $p" else "• $p : ${num(c.healthy)} → ${num(c.patient)}  (${c.sources.joinToString().ifEmpty { "effet indirect" }})"
            Text(txt, color = if (c == null) Muted else Ink, fontSize = 12.sp, fontWeight = if (c == null) FontWeight.Normal else FontWeight.SemiBold)
        }
    }
}

@Composable
private fun ChangesTable(catalog: Catalog, changes: List<ParamChange>) {
    Column(Modifier.fillMaxWidth().card().padding(12.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Text("Paramètres modifiés par rapport au chien sain de même poids", fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
        if (changes.isEmpty()) Text("Aucun : patient sain.", color = Muted, fontSize = 12.sp)
        if (changes.isNotEmpty()) {
            val head: @Composable (String, Modifier) -> Unit = { t, m -> Text(t.uppercase(), m, color = Muted, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.5.sp) }
            Row(Modifier.fillMaxWidth()) {
                head("Paramètre", Modifier.width(110.dp)); head("Bloc", Modifier.weight(1f))
                head("Sain → patient", Modifier.width(170.dp)); head("Source", Modifier.width(220.dp))
            }
            Box(Modifier.fillMaxWidth().height(1.dp).background(UiLine))
        }
        changes.forEach { c ->
            Row(Modifier.fillMaxWidth()) {
                Text(c.param, Modifier.width(110.dp), color = Ink, fontSize = 12.sp, fontWeight = FontWeight.Medium)
                Text(c.blocks.joinToString { b -> catalog.blocks.firstOrNull { it.id == b }?.label ?: b }, Modifier.weight(1f), color = Muted, fontSize = 12.sp)
                Text("${num(c.healthy)} → ${num(c.patient)}", Modifier.width(170.dp), color = Ink, fontSize = 12.sp)
                Text(c.sources.joinToString(), Modifier.width(220.dp), color = Muted, fontSize = 12.sp)
            }
        }
    }
}

/** Nombre lisible sur 4 chiffres significatifs. */
private fun num(x: Double): String {
    val a = abs(x)
    val d = when {
        a == 0.0 -> 0
        a >= 100 -> 1
        a >= 10 -> 2
        a >= 1 -> 3
        a >= 0.01 -> 4
        else -> 6
    }
    return "%.${d}f".fmt(x)
}

/** Courbe quadratique visible d'un lien : départ et arrivée au bord des blocs, point de contrôle. */
private data class Curve(val s: Offset, val c: Offset, val e: Offset) {
    fun at(t: Float): Offset {
        val u = 1 - t
        return Offset(u * u * s.x + 2 * u * t * c.x + t * t * e.x, u * u * s.y + 2 * u * t * c.y + t * t * e.y)
    }
    fun tangent(t: Float): Offset {
        val u = 1 - t
        return Offset(2 * u * (c.x - s.x) + 2 * t * (e.x - c.x), 2 * u * (c.y - s.y) + 2 * t * (e.y - c.y))
    }
}

private fun edgeCurve(a: Offset, b: Offset, box: Size, bend: Float): Curve {
    val ctrl = control(a, b, bend)
    return Curve(clip(a, ctrl, box), ctrl, clip(b, ctrl, box))
}

private data class LabelItem(val curve: Curve, val w: Float, val h: Float, val own: Int)
private data class Spot(val t: Float, val off: Float, val p: Offset, val r: Rect)

/**
 * Place les étiquettes des liens sans chevauchement : positions candidates le long de la courbe
 * (milieu d'abord) et de part et d'autre ; une position qui touche un bloc, une étiquette déjà
 * posée ou le bord est exclue ; parmi les autres, la moins traversée par les autres liens et la
 * plus proche du milieu. Les étiquettes les plus contraintes sont posées d'abord ; sans position
 * libre, l'étiquette est omise (couplage détaillé au clic sur le bloc). Renvoie (indice, centre).
 * Même algorithme que placeEdgeLabels dans web/app.js.
 */
private fun placeEdgeLabels(items: List<LabelItem>, curves: List<Curve>, boxes: List<Rect>, w: Float, h: Float): List<Pair<Int, Offset>> {
    val samples = curves.map { cv -> List(33) { cv.at(it / 32f) } }
    val ts = floatArrayOf(0.5f, 0.42f, 0.58f, 0.34f, 0.66f, 0.26f, 0.74f, 0.18f, 0.82f)
    val cands = items.map { it ->
        val list = mutableListOf<Spot>()
        for (t in ts) for (off in floatArrayOf(0f, 11f, -11f)) {
            val p = it.curve.at(t)
            val d = it.curve.tangent(t)
            val len = hypot(d.x, d.y).takeIf { l -> l > 0f } ?: 1f
            val x = p.x - d.y / len * off
            val y = p.y + d.x / len * off
            val r = Rect(x - it.w / 2, y - it.h / 2, x + it.w / 2, y + it.h / 2)
            if (r.left < 0 || r.top < 0 || r.right > w || r.bottom > h || boxes.any { b -> b.overlaps(r) }) continue
            list += Spot(t, off, Offset(x, y), r)
        }
        list
    }
    val placed = mutableListOf<Rect>()
    val out = mutableListOf<Pair<Int, Offset>>()
    for (i in items.indices.sortedWith(compareBy({ cands[it].size }, { it }))) {
        var best: Spot? = null
        var bestCost = Float.MAX_VALUE
        for (s in cands[i]) {
            if (placed.any { it.overlaps(s.r) }) continue
            var cross = 0
            samples.forEachIndexed { j, pts -> if (j != items[i].own) pts.forEach { q -> if (q.x > s.r.left && q.x < s.r.right && q.y > s.r.top && q.y < s.r.bottom) cross++ } }
            val cost = 3f * cross + 20f * abs(s.t - 0.5f) + 0.3f * abs(s.off)
            if (cost < bestCost) { bestCost = cost; best = s }
        }
        best?.let { placed += it.r; out += items[i].own to it.p }
    }
    return out
}

private fun control(a: Offset, b: Offset, bend: Float): Offset {
    val mid = Offset((a.x + b.x) / 2, (a.y + b.y) / 2)
    val len = hypot(b.x - a.x, b.y - a.y)
    val nx = -(b.y - a.y) / len
    val ny = (b.x - a.x) / len
    return Offset(mid.x + nx * bend * len, mid.y + ny * bend * len)
}

/** Point où le segment centre → `toward` sort du rectangle centré en `c`. */
private fun clip(c: Offset, toward: Offset, box: Size): Offset {
    val dx = toward.x - c.x
    val dy = toward.y - c.y
    if (dx == 0f && dy == 0f) return c
    val sx = if (dx != 0f) (box.width / 2) / abs(dx) else Float.MAX_VALUE
    val sy = if (dy != 0f) (box.height / 2) / abs(dy) else Float.MAX_VALUE
    val s = minOf(sx, sy, 1f)
    return Offset(c.x + dx * s, c.y + dy * s)
}

/** Trace un lien entre deux blocs ; renvoie la longueur visible (hors des rectangles). */
private fun DrawScope.drawEdge(a: Offset, b: Offset, box: Size, bend: Float, bidir: Boolean, color: Color, effect: PathEffect?): Float {
    val ctrl = control(a, b, bend)
    val start = clip(a, ctrl, box)
    val end = clip(b, ctrl, box)
    val path = Path().apply { moveTo(start.x, start.y); quadraticTo(ctrl.x, ctrl.y, end.x, end.y) }
    drawPath(path, color, style = Stroke(width = 1.8f, pathEffect = effect))
    arrowHead(end, ctrl, color)
    if (bidir) arrowHead(start, ctrl, color)
    return hypot(end.x - start.x, end.y - start.y)
}

private fun DrawScope.arrowHead(tip: Offset, from: Offset, color: Color) {
    val ang = atan2(tip.y - from.y, tip.x - from.x)
    val l = 11f
    val p = Path().apply {
        moveTo(tip.x, tip.y)
        lineTo(tip.x - l * cos(ang - 0.4f), tip.y - l * sin(ang - 0.4f))
        lineTo(tip.x - l * cos(ang + 0.4f), tip.y - l * sin(ang + 0.4f))
        close()
    }
    drawPath(p, color)
}
