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
import kotlinx.coroutines.delay
import kotlin.math.pow

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
    onExamine: (TreatmentPlan, Double, Death?) -> Unit,
    /** Patient déjà examiné sous traitement : le schéma modifié lui est réappliqué (même instant). */
    onReplan: (TreatmentPlan, Death?) -> Unit = { _, _ -> },
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
    var editing by remember { mutableStateOf<Int?>(null) }
    var expanded by remember { mutableStateOf<Int?>(null) }

    fun simulate() {
        if (regimens.isEmpty()) { result = null; return }
        try {
            error = null
            val plan = TreatmentPlan(regimens, days * 24.0)
            val r = controller.treatment(inputs, plan)
            result = r
            cursor = cursor.coerceIn(0.0, plan.horizonH)
            if (refDrug !in regimens.map { it.drugId }) refDrug = regimens.firstOrNull()?.drugId
            if (inputs.plan != null && inputs.atH != null) onReplan(r.plan, r.death)
        } catch (e: Exception) {
            error = e.message
        }
    }
    // Resimulation automatique à chaque changement de schéma ou de durée (plus de bouton « Simuler ») ;
    // le court délai regroupe les frappes successives dans un champ.
    var first by remember { mutableStateOf(true) }
    LaunchedEffect(regimens, days) {
        if (first) { first = false; if (!autoSimulate) return@LaunchedEffect } else delay(250)
        simulate()
    }
    fun setDose(i: Int, mg: Double) {
        if (mg < 0) return
        // les prises individuelles qui suivaient la dose nominale la suivent ; les autres restent modifiées
        regimens = regimens.mapIndexed { j, old -> if (j == i) old.copy(mgPerKg = mg, overrides = old.overrides.filterValues { it != mg }) else old }
    }

    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(
            "Exploration pédagogique de la PK/PD : les doses sont libres et n'ont valeur ni de recommandation ni de posologie. " +
                "PK de l'atropine, du bêtabloquant et de l'opioïde, et PD de tous les médicaments : placeholders (evidence D). " +
                "Ne jamais utiliser pour décider d'une dose ou d'un traitement réel.",
            color = Warn, fontSize = 12.sp,
        )
        // --- éditeur de schéma
        Column(Modifier.fillMaxWidth().card().padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
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
                    if (r.mgPerKg >= 0 && r.intervalH > 0 && r.count in 1..200) {
                        val i = editing
                        regimens = if (i == null) regimens + r else regimens.mapIndexed { j, old ->
                            // garde les ajustements des prises qui existent encore
                            if (j == i) r.copy(overrides = old.overrides.filterKeys { it < r.count }) else old
                        }
                        editing = null
                    }
                }) { Text(if (editing == null) "Ajouter" else "Enregistrer") }
                if (editing != null) OutlinedButton(onClick = { editing = null }) { Text("Annuler") }
            }
            regimens.forEachIndexed { i, r ->
                FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Box(Modifier.width(12.dp).height(12.dp).background(DrugColors[i % DrugColors.size], RoundedCornerShape(3.dp)))
                    Text("${drugs.first { it.id == r.drugId }.name} :", color = Ink, fontSize = 13.sp)
                    OutlinedButton(onClick = { setDose(i, step(r.mgPerKg, -1)) }) { Text("−", fontSize = 12.sp) }
                    // texte local : ne se recale que si la dose change ailleurs (± ou Modifier), pas pendant la frappe
                    var mgTxt by remember(i) { mutableStateOf(r.mgPerKg.toString()) }
                    LaunchedEffect(r.mgPerKg) { if (mgTxt.num() != r.mgPerKg) mgTxt = r.mgPerKg.toString() }
                    OutlinedTextField(
                        mgTxt,
                        { v -> mgTxt = v; v.num()?.let { setDose(i, it) } },
                        label = { Text("mg/kg", fontSize = 10.sp) },
                        singleLine = true,
                        modifier = Modifier.width(90.dp),
                    )
                    OutlinedButton(onClick = { setDose(i, step(r.mgPerKg, 1)) }) { Text("+", fontSize = 12.sp) }
                    Text(
                        "${if (r.route == "po") "per os" else "IV"}, " +
                            "toutes les ${r.intervalH} h × ${r.count} (de ${dayLabel(r.startH)} à ${dayLabel(r.doseTimes().last())})" +
                            (r.overrides.count { it.value == 0.0 }.takeIf { it > 0 }?.let { " ; $it omise(s)" } ?: ""),
                        color = Ink, fontSize = 13.sp,
                    )
                    OutlinedButton(onClick = {
                        editing = i; drugId = r.drugId; dose = r.mgPerKg.toString(); route = r.route
                        interval = r.intervalH.toString(); count = r.count.toString(); start = r.startH.toString()
                    }) { Text("Modifier", fontSize = 12.sp) }
                    OutlinedButton(onClick = { expanded = if (expanded == i) null else i }) {
                        Text(if (expanded == i) "Masquer les prises" else "Prises individuelles", fontSize = 12.sp)
                    }
                    OutlinedButton(onClick = { regimens = regimens.filterIndexed { j, _ -> j != i }; editing = null; expanded = null }) { Text("Retirer", fontSize = 12.sp) }
                    if (r.overrides.isNotEmpty()) Text("${r.overrides.size} prise(s) ajustée(s)", color = Warn, fontSize = 12.sp)
                }
                if (expanded == i) {
                    Text("Dose de chaque prise (mg/kg) ; 0 = prise omise", color = Muted, fontSize = 12.sp)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        r.doseTimes().forEachIndexed { k, t ->
                            var txt by remember(i, k) { mutableStateOf(r.doseAt(k).toString()) }
                            LaunchedEffect(r.doseAt(k)) { if (txt.num() != r.doseAt(k)) txt = r.doseAt(k).toString() }
                            OutlinedTextField(
                                txt,
                                { v ->
                                    txt = v
                                    v.num()?.takeIf { it >= 0 }?.let { mg ->
                                        val ov = if (mg == r.mgPerKg) r.overrides - k else r.overrides + (k to mg)
                                        regimens = regimens.mapIndexed { j, old -> if (j == i) old.copy(overrides = ov) else old }
                                    }
                                },
                                label = { Text(dayLabel(t), fontSize = 10.sp) },
                                singleLine = true,
                                modifier = Modifier.width(96.dp),
                            )
                        }
                    }
                }
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Durée simulée : ${"%.0f".fmt(days)} j", color = Muted, fontSize = 13.sp)
                Slider(value = days.toFloat(), onValueChange = { days = kotlin.math.round(it).toDouble() }, valueRange = 1f..14f, steps = 12, modifier = Modifier.width(260.dp))
                Text(if (regimens.isEmpty()) "ajouter un schéma pour simuler" else "simulation mise à jour à chaque changement", color = Muted, fontSize = 12.sp)
            }
        }
        error?.let { Text("Erreur : $it", color = Color(0xFFB00020)) }
        val r = result ?: return@Column
        val horizon = r.plan.horizonH
        val doseMarks = r.plan.regimens.mapIndexed { i, rg -> rg.administrations().map { it.first }.filter { it <= horizon } to DrugColors[i % DrugColors.size] }
        val drugColor = { id: String -> DrugColors[r.plan.regimens.indexOfFirst { it.drugId == id }.coerceAtLeast(0) % DrugColors.size] }
        // --- curseur d'examen
        Column(Modifier.fillMaxWidth().card().padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Instant d'examen : ${dayLabel(cursor)} (t = ${"%.1f".fmt(cursor)} h)", fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
                val ref = refDrug
                if (ref != null && r.conc[ref] != null) {
                    OutlinedButton(onClick = { cursor = extremum(r.t, r.conc.getValue(ref), cursor, forward = false, max = false) }) { Text("◀ creux", fontSize = 12.sp) }
                    OutlinedButton(onClick = { cursor = extremum(r.t, r.conc.getValue(ref), cursor, forward = true, max = true) }) { Text("pic ▶", fontSize = 12.sp) }
                    SmallPicker("repère : ${drugs.first { it.id == ref }.name}", r.conc.keys.map { it to drugs.first { d -> d.id == it }.name }) { refDrug = it }
                }
                Button(onClick = { onExamine(r.plan, cursor, r.death) }) { Text("Examiner le patient à cet instant") }
            }
            Slider(value = cursor.toFloat(), onValueChange = { cursor = it.toDouble() }, valueRange = 0f..horizon.toFloat())
            Text(
                r.conc.keys.joinToString("   ") { d ->
                    val i = nearest(r.t, cursor)
                    "${drugs.first { it.id == d }.name} : ${"%.4f".fmt(r.conc.getValue(d)[i].toDouble())} mg/L, E = ${"%.2f".fmt(r.effect.getValue(d)[i].toDouble())}"
                },
                color = Muted, fontSize = 12.sp,
            )
            r.death?.let { d ->
                Text("Décès du patient virtuel à ${dayLabel(d.tH)} (t = ${"%.2f".fmt(d.tH)} h, grille d'états) : ${d.label}.", color = Color(0xFFC0392B), fontSize = 13.sp)
            }
            val j = nearest(r.stateT, cursor)
            when (r.outcomes.getOrNull(j)) {
                "dead" -> Text("À cet instant : patient décédé.", color = Color(0xFF8A3B12), fontSize = 12.sp)
                "decompensated" -> Text("À cet instant : décompensé (${r.mechanisms[j].joinToString(" ; ") { MECH_FR[it] ?: it }}).", color = Color(0xFF8A3B12), fontSize = 12.sp)
                else -> {}
            }
            Text("Bandes : orange = décompensé, rouge = décès (placeholders D).", color = Muted, fontSize = 11.sp)
        }
        // bandes d'état sur la grille des états (milieux entre instants)
        val bands = r.stateT.indices.filter { r.outcomes[it] != "stable" }.map { i ->
            val t0 = if (i > 0) (r.stateT[i - 1] + r.stateT[i]) / 2 else r.stateT[i]
            val t1 = if (i < r.stateT.size - 1) (r.stateT[i] + r.stateT[i + 1]) / 2 else r.stateT[i]
            Triple(t0, t1, if (r.outcomes[i] == "dead") Color(0x38C0392B) else Color(0x2EF0A03C))
        }
        // --- courbes PK/PD
        r.conc.forEach { (d, c) ->
            val info = drugs.first { it.id == d }
            TimeChart(
                "Concentration plasmatique : ${info.name} (mg/L) ; pointillés : CE50 = ${info.ec50} mg/L",
                r.t, listOf(c to drugColor(d)), horizon, cursor, doseMarks, refLines = listOf(info.ec50.toFloat()), height = 120, bands = bands,
            ) { cursor = it }
        }
        TimeChart("Effet pharmacodynamique E(t) (0 à 1)", r.t, r.effect.map { (d, e) -> e to drugColor(d) }, horizon, cursor, doseMarks, yMin = 0f, yMax = 1f, height = 100, bands = bands) { cursor = it }
        if (r.deficitMl.any { it != 0f }) {
            TimeChart("Diurétique : déficit de volume sanguin D (mL)", r.t, listOf(r.deficitMl to Color(0xFF0B5E7A)), horizon, cursor, doseMarks, height = 90, bands = bands) { cursor = it }
            TimeChart("Diurétique : diurèse induite cumulée avec récupération U (mL/kg)", r.t, listOf(r.diuresisMlKg to Color(0xFF5B8C2A)), horizon, cursor, doseMarks, height = 90, bands = bands) { cursor = it }
            Text(
                "Débit urinaire induit u(t) = 0,25 · E(C) mL/kg/min (supplément dû au médicament ; la diurèse de base n'est pas modélisée). " +
                    "U(t) = ∫ 60 u(s) e^(−(t−s)/24 h) ds : volume induit cumulé avec oubli exponentiel (τ = 24 h). Ce n'est pas le volume " +
                    "d'urine émis, mais les pertes non encore récupérées ; il fixe les électrolytes (K⁺ − 0,04 U, Cl⁻ − 0,5 U, Na⁺ − 0,3 U, mmol/L). " +
                    "D : part prélevée sur la volémie (30 % de u), nette du retour (τ = 4 h), soustraite du volume sanguin. Paramètres PD : placeholders (D).",
                color = Muted, fontSize = 11.sp,
            )
        }
        // --- indicateurs au choix
        Column(Modifier.fillMaxWidth().card().padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
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
                r.stateT, listOf(scaled to Ink), horizon, cursor, doseMarks, height = 90, points = true, bands = bands,
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

/** Pas de ±10 % sur la dose nominale, arrondi à 3 chiffres significatifs. */
private fun step(mg: Double, dir: Int): Double {
    if (mg <= 0.0) return 0.0
    val x = if (dir > 0) mg * 1.1 else mg / 1.1
    val d = (2 - kotlin.math.floor(kotlin.math.log10(x))).toInt()
    val p = 10.0.pow(kotlin.math.abs(d))
    return if (d >= 0) kotlin.math.round(x * p) / p else kotlin.math.round(x / p) * p
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
    bands: List<Triple<Double, Double, Color>> = emptyList(),
    onTap: (Double) -> Unit,
) {
    val measurer = rememberTextMeasurer()
    Column(Modifier.fillMaxWidth().card().padding(10.dp)) {
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
            // valeurs non finies (après le décès) : ignorées
            val all = series.flatMap { it.first.asIterable() }.filter { it.isFinite() } + refLines
            if (all.isEmpty()) return@Canvas
            val lo = yMin ?: all.min()
            val hi = yMax ?: all.max()
            val span = (hi - lo).takeIf { it > 1e-9f } ?: 1f
            fun x(tt: Double) = left + (tt / horizon).toFloat() * w
            fun y(v: Float) = 4f + (1f - (v - lo) / span) * (h - 8f)
            bands.forEach { (b0, b1, c) -> drawRect(c, Offset(x(b0), 0f), androidx.compose.ui.geometry.Size(kotlin.math.max(1f, x(b1) - x(b0)), h)) }
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
            drawYAxis(measurer, lo, hi, left, size.width, h, ::y)
            refLines.forEach { v ->
                if (v in lo..hi) drawLine(Muted, Offset(left, y(v)), Offset(left + w, y(v)), pathEffect = PathEffect.dashPathEffect(floatArrayOf(6f, 6f)))
            }
            doseMarks.forEach { (times, c) -> times.forEach { tt -> drawLine(c.copy(alpha = 0.35f), Offset(x(tt), h - 10f), Offset(x(tt), h), strokeWidth = 3f) } }
            for ((v, c) in series) {
                val path = Path()
                var pen = false
                v.forEachIndexed { i, value ->
                    if (!value.isFinite()) { pen = false; return@forEachIndexed }
                    if (!pen) path.moveTo(x(t[i]), y(value)) else path.lineTo(x(t[i]), y(value))
                    pen = true
                }
                drawPath(path, c, style = Stroke(width = 1.8f))
                if (points) v.forEachIndexed { i, value -> if (value.isFinite()) drawCircle(c, 2f, Offset(x(t[i]), y(value))) }
            }
            drawLine(CursorColor, Offset(x(cursor), 0f), Offset(x(cursor), h), strokeWidth = 2f)
        }
    }
}
