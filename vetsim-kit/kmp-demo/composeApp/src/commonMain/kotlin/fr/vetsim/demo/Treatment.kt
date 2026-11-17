package fr.vetsim.demo

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Ink = Color(0xFF1B2A35)
private val Muted = Color(0xFF5B6B77)
private val Accent = Color(0xFF0B5E7A)
private val CursorColor = Color(0xFFE0465A)
private val Warn = Color(0xFF8A3B12)
private val DrugColors = listOf(Color(0xFF2563A8), Color(0xFFC0392B), Color(0xFF0B7A55), Color(0xFF7A3E9D), Color(0xFFB7791F))

/** Indicateurs suivables au long cours : clé de `eval`, libellé, unité, facteur d'affichage. */
data class Indicator(val key: String, val label: String, val unit: String, val scale: Double = 1.0)

val INDICATORS = listOf(
    Indicator("hr", "FC", "/min"), Indicator("map", "PAM", "mmHg"), Indicator("sap", "PAS", "mmHg"),
    Indicator("dap", "PAD", "mmHg"), Indicator("co", "Débit cardiaque", "L/min"), Indicator("sv", "Volume d'éjection", "mL"),
    Indicator("ef", "FE", "%", 100.0), Indicator("reg_fraction", "Fraction régurgitée", "%", 100.0),
    Indicator("la_mean", "POG moyenne", "mmHg"), Indicator("pap_mean", "PAP moyenne", "mmHg"), Indicator("cvp", "PVC", "mmHg"),
    Indicator("lv_edp", "PTDVG", "mmHg"), Indicator("edv", "VTDVG", "mL"),
    Indicator("ph", "pH artériel", ""), Indicator("pco2", "PaCO₂", "mmHg"), Indicator("pao2", "PaO₂", "mmHg"),
    Indicator("sao2", "SaO₂", "%"), Indicator("hco3", "HCO₃⁻", "mmol/L"), Indicator("be", "Excès de base", "mmol/L"),
    Indicator("lactate", "Lactate", "mmol/L"), Indicator("rr", "FR", "/min"), Indicator("po2_v", "PvO₂", "mmHg"),
    Indicator("na", "Na⁺", "mmol/L"), Indicator("k", "K⁺", "mmol/L"), Indicator("cl", "Cl⁻", "mmol/L"),
    Indicator("anion_gap", "Trou anionique", "mmol/L"), Indicator("glucose", "Glucose", "mmol/L"),
    Indicator("creat", "Créatinine", "µmol/L"), Indicator("urea", "Urée", "mmol/L"), Indicator("gfr", "DFG", "mL/min"),
    Indicator("phos", "Phosphate", "mmol/L"), Indicator("alb", "Albumine", "g/L"), Indicator("hct", "Hématocrite", "%"),
    Indicator("do2_kg", "DO₂", "mL/min/kg"), Indicator("er", "Extraction d'O₂", "%", 100.0),
    Indicator("s_tone", "Tonus sympathique", ""), Indicator("v_tone", "Tonus vagal", ""),
)

/**
 * Traitement (PK/PD) : schémas d'administration répétés sur plusieurs jours, concentrations,
 * effets, état de volume et indicateurs au choix ; curseur d'examen (pic, creux, instant
 * quelconque) qui fixe le moment où le patient est examiné dans les autres onglets.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TreatmentView(
    controller: DemoController,
    inputs: DemoInputs,
    onExamine: (TreatmentPlan, Double) -> Unit,
    initialPlan: TreatmentPlan? = null,
    initialCursorH: Double? = null,
    autoSimulate: Boolean = false,
) {
    val drugs = controller.catalog.drugs
    var regimens by remember { mutableStateOf(initialPlan?.regimens ?: inputs.plan?.regimens ?: emptyList()) }
    var days by remember { mutableStateOf(((initialPlan ?: inputs.plan)?.horizonH ?: 72.0) / 24.0) }
    var result by remember { mutableStateOf<TreatmentResult?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var cursor by remember { mutableStateOf(initialCursorH ?: inputs.atH ?: 0.0) }
    var shown by remember { mutableStateOf(listOf("hr", "map", "k")) }
    var refDrug by remember { mutableStateOf<String?>(null) }
    // éditeur de schéma
    var drugId by remember { mutableStateOf(drugs.first().id) }
    var dose by remember { mutableStateOf("1.0") }
    var route by remember { mutableStateOf("po") }
    var interval by remember { mutableStateOf("12") }
    var count by remember { mutableStateOf("6") }
    var start by remember { mutableStateOf("0") }

    fun simulate() {
        try {
            error = null
            val plan = TreatmentPlan(regimens, days * 24.0)
            result = controller.treatment(inputs, plan)
            cursor = cursor.coerceIn(0.0, plan.horizonH)
            if (refDrug !in regimens.map { it.drugId }) refDrug = regimens.firstOrNull()?.drugId
        } catch (e: Exception) {
            error = e.message
        }
    }
    if (autoSimulate) LaunchedEffect(Unit) { simulate() }

    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(
            "Exploration pédagogique de la PK/PD : les doses sont libres et n'ont valeur ni de recommandation ni de posologie. " +
                "PK de l'atropine, du bêtabloquant et de l'opioïde, et PD de tous les médicaments : placeholders (evidence D). " +
                "Ne jamais utiliser pour décider d'une dose ou d'un traitement réel.",
            color = Warn, fontSize = 12.sp,
        )
        // --- éditeur de schéma
        Column(Modifier.fillMaxWidth().background(Color.White, RoundedCornerShape(12.dp)).padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Schéma d'administration", fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                SmallPicker(drugs.first { it.id == drugId }.let { "${it.name} [${it.evidence}]" }, drugs.map { it.id to "${it.name} [${it.evidence}]" }) { drugId = it }
                Field(dose, "mg/kg", 90) { dose = it }
                SmallPicker(if (route == "po") "per os" else "IV", listOf("po" to "per os", "iv" to "IV bolus")) { route = it }
                Field(interval, "toutes les (h)", 110) { interval = it }
                Field(count, "nb de prises", 100) { count = it }
                Field(start, "début (h)", 90) { start = it }
                Button(onClick = {
                    val r = Regimen(drugId, dose.num() ?: return@Button, route, interval.num() ?: return@Button,
                        count.num()?.toInt() ?: return@Button, start.num() ?: 0.0)
                    if (r.mgPerKg >= 0 && r.intervalH > 0 && r.count in 1..200) regimens = regimens + r
                }) { Text("Ajouter") }
            }
            regimens.forEachIndexed { i, r ->
                FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Box(Modifier.width(12.dp).height(12.dp).background(DrugColors[i % DrugColors.size], RoundedCornerShape(3.dp)))
                    Text(
                        "${drugs.first { it.id == r.drugId }.name} : ${r.mgPerKg} mg/kg ${if (r.route == "po") "per os" else "IV"}, " +
                            "toutes les ${r.intervalH} h × ${r.count} (de ${dayLabel(r.startH)} à ${dayLabel(r.doseTimes().last())})",
                        color = Ink, fontSize = 13.sp,
                    )
                    OutlinedButton(onClick = { regimens = regimens.filterIndexed { j, _ -> j != i } }) { Text("Retirer", fontSize = 12.sp) }
                }
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Durée simulée : ${"%.0f".fmt(days)} j", color = Muted, fontSize = 13.sp)
                Slider(value = days.toFloat(), onValueChange = { days = kotlin.math.round(it).toDouble() }, valueRange = 1f..14f, steps = 12, modifier = Modifier.width(260.dp))
                Button(onClick = { simulate() }, enabled = regimens.isNotEmpty()) { Text("Simuler le traitement") }
            }
        }
        error?.let { Text("Erreur : $it", color = Color(0xFFB00020)) }
        val r = result ?: return@Column
        val horizon = r.plan.horizonH
        val doseMarks = r.plan.regimens.mapIndexed { i, rg -> rg.doseTimes().filter { it <= horizon } to DrugColors[i % DrugColors.size] }
        val drugColor = { id: String -> DrugColors[r.plan.regimens.indexOfFirst { it.drugId == id }.coerceAtLeast(0) % DrugColors.size] }
        // --- curseur d'examen
        Column(Modifier.fillMaxWidth().background(Color.White, RoundedCornerShape(12.dp)).padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Instant d'examen : ${dayLabel(cursor)} (t = ${"%.1f".fmt(cursor)} h)", fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
                val ref = refDrug
                if (ref != null && r.conc[ref] != null) {
                    OutlinedButton(onClick = { cursor = extremum(r.t, r.conc.getValue(ref), cursor, forward = false, max = false) }) { Text("◀ creux", fontSize = 12.sp) }
                    OutlinedButton(onClick = { cursor = extremum(r.t, r.conc.getValue(ref), cursor, forward = true, max = true) }) { Text("pic ▶", fontSize = 12.sp) }
                    SmallPicker("repère : ${drugs.first { it.id == ref }.name}", r.conc.keys.map { it to drugs.first { d -> d.id == it }.name }) { refDrug = it }
                }
                Button(
                    onClick = { onExamine(r.plan, cursor) },
                    colors = ButtonDefaults.buttonColors(containerColor = CursorColor),
                ) { Text("Examiner le patient à cet instant") }
            }
            Slider(value = cursor.toFloat(), onValueChange = { cursor = it.toDouble() }, valueRange = 0f..horizon.toFloat())
            Text(
                r.conc.keys.joinToString("   ") { d ->
                    val i = nearest(r.t, cursor)
                    "${drugs.first { it.id == d }.name} : ${"%.4f".fmt(r.conc.getValue(d)[i].toDouble())} mg/L, E = ${"%.2f".fmt(r.effect.getValue(d)[i].toDouble())}"
                },
                color = Muted, fontSize = 12.sp,
            )
        }
        // --- courbes PK/PD
        r.conc.forEach { (d, c) ->
            val info = drugs.first { it.id == d }
            TimeChart(
                "Concentration plasmatique : ${info.name} (mg/L) ; pointillés : CE50 = ${info.ec50} mg/L",
                r.t, listOf(c to drugColor(d)), horizon, cursor, doseMarks, refLines = listOf(info.ec50.toFloat()), height = 120,
            ) { cursor = it }
        }
        TimeChart("Effet pharmacodynamique E(t) (0 à 1)", r.t, r.effect.map { (d, e) -> e to drugColor(d) }, horizon, cursor, doseMarks, yMin = 0f, yMax = 1f, height = 100) { cursor = it }
        if (r.deficitMl.any { it != 0f }) {
            TimeChart("Diurétique : déficit de volume D (mL)", r.t, listOf(r.deficitMl to Color(0xFF0B5E7A)), horizon, cursor, doseMarks, height = 90) { cursor = it }
            TimeChart("Diurétique : diurèse effective cumulée U (mL/kg)", r.t, listOf(r.diuresisMlKg to Color(0xFF5B8C2A)), horizon, cursor, doseMarks, height = 90) { cursor = it }
        }
        // --- indicateurs au choix
        Column(Modifier.fillMaxWidth().background(Color.White, RoundedCornerShape(12.dp)).padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Indicateurs à suivre (cliquer pour ajouter ou retirer)", fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                INDICATORS.filter { it.key in r.states.keys }.forEach { ind ->
                    val on = ind.key in shown
                    OutlinedButton(
                        onClick = { shown = if (on) shown - ind.key else shown + ind.key },
                        colors = ButtonDefaults.outlinedButtonColors(containerColor = if (on) Accent else Color.Transparent, contentColor = if (on) Color.White else Accent),
                    ) { Text(ind.label, fontSize = 12.sp) }
                }
            }
        }
        shown.mapNotNull { k -> INDICATORS.firstOrNull { it.key == k } }.forEach { ind ->
            val v = r.states[ind.key] ?: return@forEach
            val scaled = FloatArray(v.size) { (v[it] * ind.scale).toFloat() }
            val i = nearest(r.stateT, cursor)
            TimeChart(
                "${ind.label}${if (ind.unit.isNotEmpty()) " (${ind.unit})" else ""} : ${"%.2f".fmt(scaled[i].toDouble())} à ${dayLabel(r.stateT[i])}",
                r.stateT, listOf(scaled to Ink), horizon, cursor, doseMarks, height = 90, points = true,
            ) { cursor = it }
        }
    }
}

private fun String.num(): Double? = replace(',', '.').trim().toDoubleOrNull()

private fun nearest(t: DoubleArray, x: Double): Int {
    var best = 0
    for (i in t.indices) if (kotlin.math.abs(t[i] - x) < kotlin.math.abs(t[best] - x)) best = i
    return best
}

/** Prochain maximum local (pic) après [from], ou dernier minimum local (creux) avant [from]. */
private fun extremum(t: DoubleArray, c: FloatArray, from: Double, forward: Boolean, max: Boolean): Double {
    val idx = if (forward) (1 until c.size - 1).filter { t[it] > from + 1e-9 } else (c.size - 2 downTo 1).filter { t[it] < from - 1e-9 }
    for (i in idx) {
        val isExt = if (max) c[i] >= c[i - 1] && c[i] > c[i + 1] else c[i] <= c[i - 1] && c[i] < c[i + 1]
        if (isExt) return t[i]
    }
    return from
}

@Composable
private fun Field(value: String, label: String, width: Int, onChange: (String) -> Unit) {
    OutlinedTextField(value, onChange, label = { Text(label, fontSize = 11.sp) }, singleLine = true, modifier = Modifier.width(width.dp))
}

@Composable
private fun SmallPicker(value: String, options: List<Pair<String, String>>, onPick: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        OutlinedButton(onClick = { open = true }) { Text(value, fontSize = 12.sp) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (id, text) -> DropdownMenuItem(text = { Text(text) }, onClick = { open = false; onPick(id) }) }
        }
    }
}

/** Courbe(s) sur l'horizon du traitement, graduations en jours, marques de prise, curseur ; clic = déplacer le curseur. */
@Composable
private fun TimeChart(
    title: String,
    t: DoubleArray,
    series: List<Pair<FloatArray, Color>>,
    horizon: Double,
    cursor: Double,
    doseMarks: List<Pair<List<Double>, Color>>,
    refLines: List<Float> = emptyList(),
    yMin: Float? = null,
    yMax: Float? = null,
    height: Int = 100,
    points: Boolean = false,
    onTap: (Double) -> Unit,
) {
    val measurer = rememberTextMeasurer()
    Column(Modifier.fillMaxWidth().background(Color.White, RoundedCornerShape(12.dp)).padding(10.dp)) {
        Text(title, fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 13.sp)
        Canvas(
            Modifier.fillMaxWidth().height(height.dp).pointerInput(horizon) {
                detectTapGestures { p -> onTap(((p.x - 48f) / (size.width - 48f) * horizon).coerceIn(0.0, horizon)) }
            },
        ) {
            val left = 48f
            val axis = 14f
            val w = size.width - left
            val h = size.height - axis
            val all = series.flatMap { it.first.asIterable() } + refLines
            if (all.isEmpty()) return@Canvas
            val lo = yMin ?: all.min()
            val hi = yMax ?: all.max()
            val span = (hi - lo).takeIf { it > 1e-9f } ?: 1f
            fun x(tt: Double) = left + (tt / horizon).toFloat() * w
            fun y(v: Float) = 4f + (1f - (v - lo) / span) * (h - 8f)
            // graduations : 6 h si ≤ 2 jours, sinon 1 jour
            val step = if (horizon <= 48) 6.0 else 24.0
            var g = 0.0
            while (g <= horizon + 1e-9) {
                drawLine(Color(0xFFE6EAEE), Offset(x(g), 0f), Offset(x(g), h))
                // libellé seulement s'il tient avant le bord droit
                if (x(g) + 40f < size.width) {
                    drawText(measurer, if (step < 24) dayLabel(g) else "J${(g / 24).toInt()}", Offset(x(g) + 2f, h), TextStyle(fontSize = 9.sp, color = Muted))
                }
                g += step
            }
            listOf(lo, hi).forEach { v ->
                drawText(measurer, "%.3f".fmt(v.toDouble()).trimEnd('0').trimEnd(','), Offset(0f, y(v) - 7f), TextStyle(fontSize = 9.sp, color = Muted))
            }
            refLines.forEach { v ->
                if (v in lo..hi) drawLine(Muted, Offset(left, y(v)), Offset(left + w, y(v)), pathEffect = PathEffect.dashPathEffect(floatArrayOf(6f, 6f)))
            }
            doseMarks.forEach { (times, c) -> times.forEach { tt -> drawLine(c.copy(alpha = 0.35f), Offset(x(tt), h - 10f), Offset(x(tt), h), strokeWidth = 3f) } }
            for ((v, c) in series) {
                val path = Path()
                v.forEachIndexed { i, value -> if (i == 0) path.moveTo(x(t[i]), y(value)) else path.lineTo(x(t[i]), y(value)) }
                drawPath(path, c, style = Stroke(width = 1.8f))
                if (points) v.forEachIndexed { i, value -> drawCircle(c, 2f, Offset(x(t[i]), y(value))) }
            }
            drawLine(CursorColor, Offset(x(cursor), 0f), Offset(x(cursor), h), strokeWidth = 2f)
        }
    }
}
