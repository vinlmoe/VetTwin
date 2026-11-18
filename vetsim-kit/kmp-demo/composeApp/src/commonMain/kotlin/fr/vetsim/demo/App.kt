package fr.vetsim.demo

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.border

private val Ink = Color(0xFF1B2A35)
private val Muted = Color(0xFF5B6B77)
private val EcgGreen = Color(0xFF0B7A55)
private val AoRed = Color(0xFFC0392B)
private val LvBlue = Color(0xFF2563A8)
private val LaAmber = Color(0xFFB7791F)
private val Paper = Color(0xFFFBF7F2)
private val Grid = Color(0xFFF0D9CF)

/** Écran unique de la démo. `initial` + `autoEvaluate` servent aux captures d'écran. */
@Composable
fun App(
    controller: DemoController,
    initial: DemoInputs = DemoInputs(),
    autoEvaluate: Boolean = false,
    initialTab: Int = 0,
    initialBlock: String? = null,
) {
    var tab by remember { mutableStateOf(initialTab) }
    var inputs by remember { mutableStateOf(initial) }
    var bwText by remember { mutableStateOf(initial.bw.toString().removeSuffix(".0")) }
    var result by remember { mutableStateOf<DemoResult?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    // patient de la dernière évaluation : sert à signaler un formulaire modifié mais non évalué
    var evaluated by remember { mutableStateOf<DemoInputs?>(null) }

    fun run() {
        try {
            error = null
            val bw = bwText.replace(',', '.').toDoubleOrNull() ?: throw EngineError("poids invalide")
            inputs = inputs.copy(bw = bw)
            result = controller.evaluate(inputs)
            evaluated = inputs
        } catch (e: Exception) {
            error = e.message
        }
    }
    if (autoEvaluate) LaunchedEffect(Unit) { run() }
    val dirty = evaluated?.let { e ->
        bwText.replace(',', '.').toDoubleOrNull() != e.bw || inputs.diseaseId != e.diseaseId || inputs.stage != e.stage
    } ?: false

    VetTheme {
        Surface(Modifier.fillMaxSize(), color = UiBg) {
            Row(Modifier.fillMaxSize().padding(16.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                // ---------------------------------------------------------------- commandes
                Column(
                    Modifier.width(280.dp).verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Text("VetTwins", fontSize = 26.sp, fontWeight = FontWeight.Bold, color = Ink)
                    Text("Patient virtuel canin · moteur vetsim, API ${controller.apiVersion}", color = Muted, fontSize = 12.sp)
                    OutlinedTextField(bwText, { bwText = it }, label = { Text("Poids (kg)") }, singleLine = true)
                    Picker(
                        label = "Maladie",
                        value = controller.catalog.diseases.firstOrNull { it.id == inputs.diseaseId }?.name ?: "Aucune (sain)",
                        options = listOf(null to "Aucune (sain)") + controller.catalog.diseases.map { it.id to "${it.name} [${it.evidence}]" },
                    ) { id -> inputs = inputs.copy(diseaseId = id, stage = controller.catalog.diseases.firstOrNull { it.id == id }?.stages?.firstOrNull()?.name) }
                    val disease = controller.catalog.diseases.firstOrNull { it.id == inputs.diseaseId }
                    if (disease != null) {
                        val stageOptions = disease.stages.map { it.name to "${it.name} (s ${it.lo}–${it.hi})" }
                        Picker("Stade", stageOptions.firstOrNull { it.first == inputs.stage }?.second ?: inputs.stage ?: "", stageOptions) {
                            inputs = inputs.copy(stage = it)
                        }
                    }
                    Button(
                        onClick = { run() },
                        modifier = Modifier.fillMaxWidth()
                            .then(if (dirty) Modifier.border(3.dp, Color(0xFFF0B46A), androidx.compose.foundation.shape.RoundedCornerShape(50)) else Modifier),
                    ) { Text(if (dirty) "Évaluer ce nouveau patient" else "Évaluer") }
                    Spacer(Modifier.height(4.dp))
                    val plan = inputs.plan
                    if (plan != null && inputs.atH != null) {
                        Text(
                            "Traitement en cours (${plan.regimens.size} schéma${if (plan.regimens.size > 1) "s" else ""}) : " +
                                "patient examiné à ${dayLabel(inputs.atH!!)}.",
                            color = Color(0xFFC0392B), fontSize = 12.sp,
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            OutlinedButton(onClick = { tab = 4 }) { Text("Modifier", fontSize = 12.sp) }
                            OutlinedButton(onClick = { inputs = inputs.copy(plan = null, atH = null, death = null); run() }) { Text("Arrêter", fontSize = 12.sp) }
                        }
                    }
                    Text(
                        "Médicaments : onglet « Traitement (PK/PD) », puis « Examiner le patient à cet instant ». " +
                            "Les concentrations découlent toujours des doses (PK/PD).",
                        color = Muted, fontSize = 12.sp,
                    )
                    Text(
                        "Outil pédagogique : ne jamais utiliser pour décider d'une dose ou d'un traitement réel. " +
                            "Les niveaux de preuve [A–D] sont ceux des modules ; D = placeholder.",
                        color = Color(0xFF8A3B12), fontSize = 12.sp,
                    )
                }
                // ---------------------------------------------------------------- résultats
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                // en-tête du cas et onglets : hors de la zone qui défile, toujours visibles
                val r = result
                if (r == null) {
                    Text("Choisir un patient puis « Évaluer ».", color = Muted)
                } else {
                    Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                        Text(r.caseLabel, fontSize = 18.sp, fontWeight = FontWeight.SemiBold, color = Ink)
                        if (dirty) Text("  · formulaire modifié, non évalué", fontSize = 13.sp, color = Color(0xFF8A3B12))
                    }
                    Tabs(listOf("Patient", "Respiration", "Schéma du modèle", "Auscultation", "Traitement (PK/PD)"), tab) { tab = it }
                    Box(Modifier.fillMaxWidth().height(1.dp).background(UiLine))
                }
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    error?.let { Text("Erreur : $it", color = Color(0xFFB00020)) }
                    if (r != null) OutcomeBanner(r.outcome)
                    if (r != null && tab == 1) { if (r.outcome.dead) DeadView("respiration") else RespirationView(r) }
                    if (tab == 2) ModelView(controller.catalog, r, initialBlock)
                    if (r != null && tab == 4) {
                        TreatmentView(controller, inputs, onExamine = { plan, t, death ->
                            inputs = inputs.copy(plan = plan, atH = t, death = death); run(); tab = 0
                        }, onReplan = { plan, death ->
                            inputs = inputs.copy(plan = plan, atH = inputs.atH?.coerceAtMost(plan.horizonH), death = death); run()
                        })
                    }
                    if (r != null && tab == 3) {
                        // clé : nouvelle synthèse quand le patient change
                        if (r.outcome.dead) DeadView("bruit cardiaque : silence à l'auscultation")
                        else androidx.compose.runtime.key(inputs) { AuscultationView(controller, inputs) }
                    }
                    if (r != null && tab == 0 && r.outcome.dead) DeadView("ECG ni de pression pulsée")
                    if (r != null && tab == 0 && !r.outcome.dead) {
                        Vitals(r)
                        Panel("ECG, dérivation II (${(r.ecg.size / r.ecgFs).toInt()} s, ${"%.0f".fmt(r.values["hr"] ?: 0.0)} /min ; échelle fixe −1,5 à +2,5 mV)") {
                            // échelle unique de l'ECG (−1,5 à +2,5 mV) : amplitudes comparables entre patients
                            Trace(listOf(r.ecg to EcgGreen), paper = true, height = 150, yMin = -1.5f, yMax = 2.5f, durationS = r.ecg.size / r.ecgFs)
                        }
                        Panel("Pressions (mmHg) : aorte, VG, OG, sur le même temps que l'ECG (simulées battement par battement)") {
                            if (r.alignedAo.isNotEmpty()) {
                                // même fenêtre de temps que l'ECG : chaque QRS est suivi de la montée de pression du VG
                                Trace(
                                    listOf(r.alignedAo to AoRed, r.alignedLv to LvBlue, r.alignedLa to LaAmber), height = 170, yMin = 0f,
                                    durationS = r.alignedAo.size / r.alignedFs,
                                )
                            } else {
                                val two = { a: FloatArray -> a + a }
                                Trace(
                                    listOf(two(r.ao) to AoRed, two(r.lv) to LvBlue, two(r.la) to LaAmber), height = 170, yMin = 0f,
                                    durationS = 2.0 * r.ao.size * r.pressureDt,
                                )
                            }
                            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                                Legend("Aorte", AoRed); Legend("VG", LvBlue); Legend("OG", LaAmber)
                                Text("PCG : ${r.pcgLabel}", color = Muted, fontSize = 13.sp)
                            }
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Box(Modifier.weight(1f)) { ValuesTable("Hémodynamique", HEMO, r.values) }
                            Box(Modifier.weight(1f)) { ValuesTable("Gaz du sang", GASES, r.values) }
                            Box(Modifier.weight(1f)) { ValuesTable("Biochimie", CHEM, r.values) }
                        }
                        Panel("Signes émergents") {
                            if (r.signs.isEmpty()) Text("Aucun signe dérivé au-delà des seuils sourcés.", color = Muted)
                            r.signs.forEach {
                                Text("• ${it.label}", color = Ink, fontSize = 13.sp)
                                Text("   ${it.source}", color = Muted, fontSize = 11.sp)
                            }
                        }
                    }
                }
                }
            }
        }
    }
}

/** Bandeau d'issue : décompensation (mécanismes) ou décès (cause). Seuils et pentes : placeholders D. */
@Composable
fun OutcomeBanner(o: Outcome) {
    if (o.state == "stable") return
    val dead = o.dead
    Column(
        Modifier.fillMaxWidth()
            .background(if (dead) Color(0xFF3A1114) else Color(0xFFFFF4E5), androidx.compose.foundation.shape.RoundedCornerShape(10.dp))
            .padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        val fg = if (dead) Color(0xFFFBE9EA) else Color(0xFF6B3D00)
        if (dead) {
            Text("Décès du patient virtuel : ${o.label ?: "décès"}" + (o.sinceH?.let { " (survenu à ${dayLabel(it)})" } ?: ""), color = fg, fontWeight = FontWeight.SemiBold)
            Text("Aucun état physiologique viable n'existe pour ce patient.", color = fg, fontSize = 13.sp)
        } else {
            Text("Patient décompensé", color = fg, fontWeight = FontWeight.SemiBold)
            o.mechanisms.forEach { m ->
                Text("• ${m.label} : ${"%.2f".fmt(m.value)} (seuil ${"%.2f".fmt(m.threshold)})" + if (m.factor < 1) ", facteur ${"%.2f".fmt(m.factor)}" else "", color = fg, fontSize = 13.sp)
            }
        }
        Text("Mécanismes physiologiques du modèle ; seuils et pentes : placeholders (evidence D).", color = fg, fontSize = 11.sp)
    }
}

/** Vue d'un patient décédé (pas de signaux). */
@Composable
fun DeadView(what: String) {
    Panel("Patient décédé") { Text("Pas de $what.", color = Muted) }
}

private data class Row3(val key: String, val label: String, val unit: String, val fmt: String, val scale: Double = 1.0)

private val HEMO = listOf(
    Row3("hr", "FC", "/min", "%.0f"), Row3("sap", "PAS", "mmHg", "%.0f"), Row3("dap", "PAD", "mmHg", "%.0f"),
    Row3("map", "PAM", "mmHg", "%.0f"), Row3("co", "Débit cardiaque", "L/min", "%.2f"), Row3("ef", "FE", "%", "%.0f", 100.0),
    Row3("reg_fraction", "Fraction régurgitée", "%", "%.0f", 100.0), Row3("la_mean", "POG moyenne", "mmHg", "%.1f"),
    Row3("pap_mean", "PAP moyenne", "mmHg", "%.1f"), Row3("cvp", "PVC", "mmHg", "%.1f"),
)
private val GASES = listOf(
    Row3("ph", "pH artériel", "", "%.3f"), Row3("pco2", "PaCO₂", "mmHg", "%.1f"), Row3("pao2", "PaO₂", "mmHg", "%.0f"),
    Row3("hco3", "HCO₃⁻", "mmol/L", "%.1f"), Row3("be", "Excès de base", "mmol/L", "%.1f"), Row3("sao2", "SaO₂", "%", "%.1f"),
    Row3("lactate", "Lactate", "mmol/L", "%.2f"), Row3("ph_v", "pH veineux mêlé", "", "%.3f"),
    Row3("pco2_v", "PvCO₂", "mmHg", "%.1f"), Row3("po2_v", "PvO₂", "mmHg", "%.0f"),
)
private val CHEM = listOf(
    Row3("na", "Na⁺", "mmol/L", "%.0f"), Row3("k", "K⁺", "mmol/L", "%.1f"), Row3("cl", "Cl⁻", "mmol/L", "%.0f"),
    Row3("anion_gap", "Trou anionique", "mmol/L", "%.1f"), Row3("glucose", "Glucose", "mmol/L", "%.1f"),
    Row3("creat", "Créatinine", "µmol/L", "%.0f"), Row3("urea", "Urée", "mmol/L", "%.1f"),
    Row3("phos", "Phosphate", "mmol/L", "%.2f"), Row3("alb", "Albumine", "g/L", "%.1f"), Row3("hct", "Hématocrite", "%", "%.0f"),
)

@Composable
private fun Tabs(titles: List<String>, selected: Int, onSelect: (Int) -> Unit) {
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        titles.forEachIndexed { i, t ->
            val on = i == selected
            OutlinedButton(
                onClick = { onSelect(i) },
                modifier = Modifier.semantics { role = Role.Tab; this.selected = on },
                colors = androidx.compose.material3.ButtonDefaults.outlinedButtonColors(
                    containerColor = if (on) Color(0xFF0B5E7A) else Color.Transparent,
                    contentColor = if (on) Color.White else Color(0xFF0B5E7A),
                ),
            ) { Text(t) }
        }
    }
}

@Composable
private fun ValuesTable(title: String, rows: List<Row3>, values: Map<String, Double>) {
    Panel(title) {
        Column {
            rows.forEachIndexed { i, r ->
                if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Color(0xFFEEF1F4)))
                Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                    Text(r.label, Modifier.weight(1f), color = Muted, fontSize = 13.sp)
                    Text(r.fmt.fmt((values[r.key] ?: Double.NaN) * r.scale), color = Ink, fontSize = 13.sp, fontWeight = FontWeight.Medium, style = Tabular)
                    Text(" ${r.unit}", Modifier.widthIn(min = 52.dp), color = Muted, fontSize = 11.sp)
                }
            }
        }
    }
}

/** Chiffres à chasse fixe : colonnes de valeurs alignées. */
private val Tabular = androidx.compose.ui.text.TextStyle(fontFeatureSettings = "tnum")

/** Constantes clés en tête de l'onglet Patient (valeurs du modèle, sans intervalle de référence). */
@Composable
private fun Vitals(r: DemoResult) {
    val v = { k: String -> r.values[k] ?: Double.NaN }
    val tiles = listOf(
        Triple("FC", "%.0f".fmt(v("hr")), "/min"),
        Triple("PAS / PAD (mmHg)", "${"%.0f".fmt(v("sap"))}/${"%.0f".fmt(v("dap"))}", ""),
        Triple("PAM", "%.0f".fmt(v("map")), "mmHg"),
        Triple("FR", "%.0f".fmt(r.values["rr"] ?: r.respiration.rrPerMin), "/min"),
        Triple("SaO₂", "%.1f".fmt(v("sao2")), "%"),
        Triple("Débit cardiaque", "%.2f".fmt(v("co")), "L/min"),
        Triple("Lactate", "%.2f".fmt(v("lactate")), "mmol/L"),
    )
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        tiles.forEach { (k, value, u) -> VitalTile(k, value, u, Modifier.weight(1f)) }
    }
}

@Composable
private fun Panel(title: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxWidth().card().padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(title, fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
        content()
    }
}

@Composable
private fun Legend(label: String, color: Color) {
    Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
        Box(Modifier.width(16.dp).height(3.dp).background(color))
        Text(" $label", color = Muted, fontSize = 12.sp)
    }
}

/**
 * Tracé de séries régulièrement échantillonnées (même durée), échelle commune ; axe vertical
 * gradué dans la marge gauche et, si `durationS` est donné, axe de temps en secondes.
 */
@Composable
private fun Trace(
    series: List<Pair<FloatArray, Color>>, height: Int, paper: Boolean = false, yMin: Float? = null, yMax: Float? = null,
    durationS: Double? = null,
) {
    val measurer = rememberTextMeasurer()
    Canvas(Modifier.fillMaxWidth().height(height.dp)) {
        val left = 40f
        val axis = if (durationS != null) 16f else 0f
        val w = size.width - left
        val h = size.height - axis
        if (paper) {
            drawRect(Paper, Offset(left, 0f), androidx.compose.ui.geometry.Size(w, h))
            val step = w / 40f
            var x = left
            while (x < size.width) { drawLine(Grid, Offset(x, 0f), Offset(x, h)); x += step }
            var y = 0f
            while (y < h) { drawLine(Grid, Offset(left, y), Offset(size.width, y)); y += step }
        }
        val all = series.flatMap { it.first.asIterable() }
        if (all.isEmpty()) return@Canvas
        val lo = yMin ?: all.min()
        val hi = yMax ?: all.max()
        val span = (hi - lo).takeIf { it > 1e-6f } ?: 1f
        val pad = 6f
        fun yOf(v: Float) = pad + (1f - (v - lo) / span) * (h - 2 * pad)
        drawYAxis(measurer, lo, hi, left, size.width, h, ::yOf, grid = !paper)
        if (durationS != null && durationS > 0) {
            val stepS = when { durationS <= 1.5 -> 0.25; durationS <= 3 -> 0.5; else -> 1.0 }
            var t = 0.0
            while (t <= durationS + 1e-9) {
                val x = left + (t / durationS).toFloat() * w
                if (!paper) drawLine(UiGrid, Offset(x, 0f), Offset(x, h))
                if (x + 36f < size.width) {
                    drawText(measurer, "${"%.2f".fmt(t).trimEnd('0').trimEnd(',')} s", Offset(x + 2f, h + 1f), TextStyle(fontSize = 9.sp, color = Muted))
                }
                t += stepS
            }
        }
        for ((v, color) in series) {
            val path = Path()
            v.forEachIndexed { i, y ->
                val px = left + i.toFloat() / (v.size - 1).coerceAtLeast(1) * w
                if (i == 0) path.moveTo(px, yOf(y)) else path.lineTo(px, yOf(y))
            }
            drawPath(path, color, style = Stroke(width = 1.6f))
        }
    }
}

@Composable
private fun Picker(label: String, value: String, options: List<Pair<String?, String>>, onPick: (String?) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Column {
        Text(label, color = Muted, fontSize = 12.sp)
        Box {
            OutlinedButton(onClick = { open = true }, modifier = Modifier.fillMaxWidth()) { Text(value) }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                options.forEach { (id, text) ->
                    DropdownMenuItem(text = { Text(text) }, onClick = { open = false; onPick(id) })
                }
            }
        }
    }
}
