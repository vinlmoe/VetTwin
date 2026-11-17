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
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.lightColorScheme
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

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
    var bwText by remember { mutableStateOf(initial.bw.toString()) }
    var result by remember { mutableStateOf<DemoResult?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    fun run() {
        try {
            error = null
            val bw = bwText.replace(',', '.').toDoubleOrNull() ?: throw EngineError("poids invalide")
            inputs = inputs.copy(bw = bw)
            result = controller.evaluate(inputs)
        } catch (e: Exception) {
            error = e.message
        }
    }
    if (autoEvaluate) LaunchedEffect(Unit) { run() }

    MaterialTheme(colorScheme = lightColorScheme(primary = Color(0xFF0B5E7A))) {
        Surface(Modifier.fillMaxSize(), color = Color(0xFFF6F7F8)) {
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
                        Picker("Stade", inputs.stage ?: "", disease.stages.map { it.name to "${it.name} (s ${it.lo}–${it.hi})" }) {
                            inputs = inputs.copy(stage = it)
                        }
                    }
                    Button(onClick = { run() }, modifier = Modifier.fillMaxWidth()) { Text("Évaluer") }
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
                        "Médicaments : « Traitement répété (PK/PD) », puis « Examiner le patient à cet instant ». " +
                            "Les concentrations découlent toujours des doses (PK/PD).",
                        color = Muted, fontSize = 12.sp,
                    )
                    OutlinedButton(onClick = { tab = 4 }, enabled = result != null, modifier = Modifier.fillMaxWidth()) {
                        Text("Traitement répété (PK/PD)")
                    }
                    OutlinedButton(
                        onClick = { tab = 3 },
                        enabled = result != null,
                        modifier = Modifier.fillMaxWidth(),
                    ) { Text("Ausculter (son, ECG, pressions)") }
                    Text(
                        "Outil pédagogique : ne jamais utiliser pour décider d'une dose ou d'un traitement réel. " +
                            "Les niveaux de preuve [A–D] sont ceux des modules ; D = placeholder.",
                        color = Color(0xFF8A3B12), fontSize = 12.sp,
                    )
                }
                // ---------------------------------------------------------------- résultats
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    error?.let { Text("Erreur : $it", color = Color(0xFFB00020)) }
                    val r = result
                    if (r == null) {
                        Text("Choisir un patient puis « Évaluer ».", color = Muted)
                    } else {
                        Text(r.caseLabel, fontSize = 18.sp, fontWeight = FontWeight.SemiBold, color = Ink)
                        OutcomeBanner(r.outcome)
                        Tabs(listOf("Patient", "Respiration", "Schéma du modèle", "Auscultation", "Traitement (PK/PD)"), tab) { tab = it }
                    }
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
                        Panel("ECG, dérivation II (${(r.ecg.size / r.ecgFs).toInt()} s, ${"%.0f".fmt(r.values["hr"] ?: 0.0)} /min ; échelle fixe −1,5 à +2,5 mV)") {
                            // échelle unique de l'ECG (−1,5 à +2,5 mV) : amplitudes comparables entre patients
                            Trace(listOf(r.ecg to EcgGreen), paper = true, height = 150, yMin = -1.5f, yMax = 2.5f)
                        }
                        Panel("Pressions sur deux battements (mmHg) : aorte, VG, OG") {
                            val two = { a: FloatArray -> a + a }
                            Trace(listOf(two(r.ao) to AoRed, two(r.lv) to LvBlue, two(r.la) to LaAmber), height = 170, yMin = 0f)
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
                                Text("• ${it.label}", color = Ink, fontSize = 14.sp)
                                Text("   ${it.source}", color = Muted, fontSize = 11.sp)
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
        rows.forEach { r ->
            Row(Modifier.fillMaxWidth().padding(vertical = 1.dp)) {
                Text(r.label, Modifier.weight(1f), color = Muted, fontSize = 13.sp)
                Text(r.fmt.fmt((values[r.key] ?: Double.NaN) * r.scale), color = Ink, fontSize = 13.sp, fontWeight = FontWeight.Medium)
                Text(" ${r.unit}", Modifier.widthIn(min = 48.dp), color = Muted, fontSize = 11.sp)
            }
        }
    }
}

@Composable
private fun Panel(title: String, content: @Composable () -> Unit) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.fillMaxWidth().background(Color.White).padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(title, fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 14.sp)
            content()
        }
    }
}

@Composable
private fun Legend(label: String, color: Color) {
    Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
        Box(Modifier.width(16.dp).height(3.dp).background(color))
        Text(" $label", color = Muted, fontSize = 12.sp)
    }
}

/** Tracé de séries régulièrement échantillonnées (même durée), échelle commune. */
@Composable
private fun Trace(series: List<Pair<FloatArray, Color>>, height: Int, paper: Boolean = false, yMin: Float? = null, yMax: Float? = null) {
    Canvas(Modifier.fillMaxWidth().height(height.dp).background(if (paper) Paper else Color.White)) {
        if (paper) {
            val step = size.width / 40f
            var x = 0f
            while (x < size.width) { drawLine(Grid, Offset(x, 0f), Offset(x, size.height)); x += step }
            var y = 0f
            while (y < size.height) { drawLine(Grid, Offset(0f, y), Offset(size.width, y)); y += step }
        }
        val all = series.flatMap { it.first.asIterable() }
        if (all.isEmpty()) return@Canvas
        val lo = yMin ?: all.min()
        val hi = yMax ?: all.max()
        val span = (hi - lo).takeIf { it > 1e-6f } ?: 1f
        val pad = 6f
        for ((v, color) in series) {
            val path = Path()
            v.forEachIndexed { i, y ->
                val px = i.toFloat() / (v.size - 1).coerceAtLeast(1) * size.width
                val py = pad + (1f - (y - lo) / span) * (size.height - 2 * pad)
                if (i == 0) path.moveTo(px, py) else path.lineTo(px, py)
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
