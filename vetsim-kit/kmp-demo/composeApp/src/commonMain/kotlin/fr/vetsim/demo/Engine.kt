package fr.vetsim.demo

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** API par messages JSON du moteur (docs/KMP.md) : chaînes JSON en entrée et en sortie. */
interface VetsimEngine {
    fun apiVersion(): String
    fun loadModules(json: String): String
    fun newPatient(optionsJson: String): String
    fun addDisease(patientId: String, optionsJson: String): String
    fun state(patientId: String, optionsJson: String): String
    fun signals(patientId: String, optionsJson: String): String
    fun timecourse(patientId: String, dosesJson: String): String
    fun listModules(): String
    fun pcg(patientId: String, optionsJson: String): String
}

/** Moteur natif (Rust via UniFFI sur Android et JVM). */
expect fun createEngine(): VetsimEngine

/** Joue un signal audio mono (PCG), normalisé dans [-1, 1]. */
expect fun playAudio(samples: FloatArray, sampleRate: Int)

/** Arrête le son en cours (sans effet s'il n'y en a pas). */
expect fun stopAudio()

data class StageInfo(val name: String, val lo: Double, val hi: Double)
/** Effet déclaré par un module : paramètre ciblé, opération, blocs du modèle concernés. */
data class EffectInfo(val param: String, val op: String, val conditional: Boolean, val blocks: List<String>)
data class DiseaseInfo(val id: String, val name: String, val evidence: String, val stages: List<StageInfo>, val effects: List<EffectInfo>)
data class DrugInfo(val id: String, val name: String, val evidence: String, val ec50: Double, val effects: List<EffectInfo>)
/** Bloc du modèle (vetsim_core::blocks) et paramètres qui y interviennent. */
data class BlockInfo(val id: String, val label: String, val params: List<String>)
data class Catalog(val diseases: List<DiseaseInfo>, val drugs: List<DrugInfo>, val blocks: List<BlockInfo>)

/** Paramètre modifié par rapport au chien sain de même poids, avec le module qui le déclare. */
data class ParamChange(val param: String, val healthy: Double, val patient: Double, val blocks: List<String>, val sources: List<String>)

/** Courbes respiratoires du modèle autonome (même fenêtre que l'ECG). */
data class Respiration(
    val fs: Double,
    val rrPerMin: Double,
    val vtAlvMl: Double,
    val volume: FloatArray,
    val flow: FloatArray,
    val hr: FloatArray,
    val map: FloatArray,
    val sap: FloatArray,
    val dap: FloatArray,
)

data class Sign(val label: String, val source: String)

/** Schéma d'administration : [count] prises de [mgPerKg] mg/kg toutes les [intervalH] h dès [startH]. */
data class Regimen(
    val drugId: String,
    val mgPerKg: Double,
    val route: String,
    val intervalH: Double,
    val count: Int,
    val startH: Double = 0.0,
    /** Ajustements prise par prise : indice → mg/kg (0 = prise omise). */
    val overrides: Map<Int, Double> = emptyMap(),
) {
    fun doseTimes(): List<Double> = List(count) { startH + it * intervalH }

    /** Dose de la prise [i] (mg/kg), après ajustement éventuel. */
    fun doseAt(i: Int): Double = overrides[i] ?: mgPerKg

    /** Prises effectives (instant, mg/kg), sans les prises omises. */
    fun administrations(): List<Pair<Double, Double>> =
        doseTimes().mapIndexed { i, t -> t to doseAt(i) }.filter { it.second > 0.0 }
}

/** Traitement : un ou plusieurs schémas, simulés sur [horizonH] heures. */
data class TreatmentPlan(val regimens: List<Regimen>, val horizonH: Double) {
    fun dosesJson(): String = regimens.flatMap { r ->
        r.administrations().map { (t, mg) -> """{"t_h": $t, "drug": "${r.drugId}", "mg_per_kg": $mg, "route": "${r.route}"}""" }
    }.joinToString(", ", "[", "]")
}

data class DemoInputs(
    val bw: Double = 20.0,
    val diseaseId: String? = null,
    val stage: String? = null,
    val seed: Long = 1,
    /** Traitement en cours et instant d'examen (h) : seule voie d'exposition aux médicaments (PK/PD). */
    val plan: TreatmentPlan? = null,
    val atH: Double? = null,
    /** Décès daté par la simulation du traitement (absorbant pour tout examen postérieur). */
    val death: Death? = null,
) {
    /** Options d'exposition pour state/pcg : schéma à l'instant t ; sans traitement, aucun médicament. */
    fun exposureJson(): String = if (plan != null && atH != null && plan.regimens.isNotEmpty()) {
        """"doses": ${plan.dosesJson()}, "at_h": $atH"""
    } else {
        """"conc": {}"""
    }

    /** Patient examiné après le décès daté par la simulation : il reste décédé. */
    val deadAtExam: Boolean get() = death != null && atH != null && atH >= death.tH
}

/** Décès daté par une simulation de traitement. */
data class Death(val tH: Double, val cause: String, val label: String)

/** Mécanisme de décompensation actif (seuils et pentes : placeholders D). */
data class Mechanism(val id: String, val label: String, val value: Double, val threshold: Double, val factor: Double)

/** Issue mécanistique : « stable », « decompensated » ou « dead ». */
data class Outcome(val state: String, val mechanisms: List<Mechanism> = emptyList(), val cause: String? = null, val label: String? = null, val sinceH: Double? = null) {
    val dead: Boolean get() = state == "dead"
}

/** Libellés des mécanismes et causes de décès (identiques à l'API). */
val MECH_FR = mapOf(
    "acidemia_depression" to "dépression myocardique et vasoplégie par acidémie",
    "hypoxic_depression" to "dépression myocardique par hypoxie tissulaire",
    "coronary_ischemia" to "ischémie myocardique (pression de perfusion coronaire basse)",
    "hypokalemic_weakness" to "faiblesse des muscles respiratoires (hypokaliémie)",
    "atrial_standstill" to "arrêt atrial hyperkaliémique (pas d'onde P ni de contraction atriale)",
    "conduction_block" to "trouble de conduction (FC ventriculaire inférieure à la FC sinusale)",
    "anaerobic_metabolism" to "métabolisme anaérobie (extraction d'O₂ élevée, lactate)",
    "pulmonary_edema" to "œdème pulmonaire (POG élevée)",
    "renal_hypoperfusion" to "hypoperfusion rénale (PAM basse)",
    "baroreflex_exhausted" to "baroréflexe épuisé (tonus sympathique maximal)",
)
val DEATH_FR = mapOf(
    "cardiac_arrest" to "arrêt cardiaque",
    "circulatory_collapse" to "effondrement circulatoire",
    "irreversible_shock" to "choc irréversible",
    "oxygen_debt" to "dette d'oxygène (consommation > apport)",
    "no_steady_state" to "aucun état stationnaire viable",
    "vicious_circle" to "spirale de décompensation non résolue",
)

/** Libellé d'instant : « J2 14 h » (J0 = jour de la première prise). */
fun dayLabel(h: Double): String {
    val d = kotlin.math.floor(h / 24).toInt()
    val r = h - d * 24
    val hh = kotlin.math.floor(r).toInt()
    val mm = kotlin.math.round((r - hh) * 60).toInt()
    return "J$d ${hh} h" + if (mm in 1..59) " ${mm.toString().padStart(2, '0')}" else ""
}

/** Résultat d'une simulation de traitement. */
class TreatmentResult(
    val t: DoubleArray,
    val conc: Map<String, FloatArray>,
    val effect: Map<String, FloatArray>,
    val deficitMl: FloatArray,
    val diuresisMlKg: FloatArray,
    val stateT: DoubleArray,
    /** Indicateurs aux instants `stateT` ; NaN après le décès (absorbant). */
    val states: Map<String, FloatArray>,
    /** Issue à chaque instant de `stateT` : « stable », « decompensated », « dead ». */
    val outcomes: List<String>,
    /** Mécanismes actifs à chaque instant de `stateT`. */
    val mechanisms: List<List<String>>,
    /** Premier instant de décès (h) et sa cause, ou null. */
    val death: Death?,
    val plan: TreatmentPlan,
)

data class DemoResult(
    val values: Map<String, Double>,
    val caseLabel: String,
    val signs: List<Sign>,
    val ecg: FloatArray,
    val ecgFs: Double,
    val pcg: FloatArray,
    val pcgFs: Int,
    val pcgLabel: String,
    val pressureDt: Double,
    val ao: FloatArray,
    val lv: FloatArray,
    val la: FloatArray,
    /** Pressions simulées battement par battement, alignées sur l'ECG (même fenêtre de temps). */
    val alignedFs: Double,
    val alignedAo: FloatArray,
    val alignedLv: FloatArray,
    val alignedLa: FloatArray,
    val respiration: Respiration,
    val changes: List<ParamChange>,
    val diseaseId: String?,
    val drugIds: List<String>,
    /** Issue mécanistique ; si décès, pas de signaux (tableaux vides). */
    val outcome: Outcome,
)

/** Auscultation : son, ECG et pressions sur le même axe de temps. */
class AuscultationResult(
    val samples: FloatArray,
    val fs: Int,
    val durationS: Double,
    val grade: Int,
    val murmur: String,
    val s3: Boolean,
    val hr: Double,
    val site: String,
    val beats: DoubleArray,
    val ecg: FloatArray,
    val ecgFs: Double,
    val pressureFs: Double,
    val ao: FloatArray,
    val lv: FloatArray,
    val la: FloatArray,
) {
    /** Spectrogramme (puissance en dB) : [trame][classe], 0–[maxHz] Hz, trames de [hop] échantillons. */
    val spectrogram: Spectrogram by lazy { Spectrogram.of(samples, fs, nfft = 512, hop = 64, maxHz = 500.0) }
}

class EngineError(message: String) : Exception(message)

private fun JsonElement.d(): Double = jsonPrimitive.doubleOrNull ?: Double.NaN
private fun JsonElement.floats(): FloatArray = jsonArray.map { it.jsonPrimitive.doubleOrNull?.toFloat() ?: 0f }.toFloatArray()
private fun JsonObject.str(k: String): String = (this[k] as? JsonPrimitive)?.content ?: ""
private fun effects(e: JsonElement?): List<EffectInfo> = (e as? JsonArray)?.map {
    val o = it.jsonObject
    EffectInfo(o.str("param"), o.str("op"), o["conditional"]?.jsonPrimitive?.booleanOrNull == true,
        o["blocks"]!!.jsonArray.map { b -> b.jsonPrimitive.content })
} ?: emptyList()

/** Pilote la démo : charge les modules, crée un patient, évalue l'état et les signaux. */
class DemoController(private val engine: VetsimEngine = createEngine()) {
    private val json = Json { ignoreUnknownKeys = true }
    val apiVersion: String = engine.apiVersion()
    val catalog: Catalog

    init {
        check(engine.loadModules(MODULES_BUNDLE))
        val c = check(engine.listModules())
        catalog = Catalog(
            diseases = c["diseases"]!!.jsonArray.map { d ->
                val o = d.jsonObject
                DiseaseInfo(
                    id = o.str("id"), name = o.str("name"), evidence = o.str("evidence"),
                    stages = o["stages"]!!.jsonArray.map { s ->
                        val so = s.jsonObject
                        StageInfo(so.str("name"), so["lo"]!!.d(), so["hi"]!!.d())
                    },
                    effects = effects(o["effects"]),
                )
            },
            drugs = c["drugs"]!!.jsonArray.map { d ->
                val o = d.jsonObject
                DrugInfo(o.str("id"), o.str("name"), o.str("evidence"), o["ec50_mg_L"]!!.d(), effects(o["effects"]))
            },
            blocks = c["blocks"]!!.jsonArray.map { b ->
                val o = b.jsonObject
                BlockInfo(o.str("id"), o.str("label_fr"), o["params"]!!.jsonArray.map { it.jsonPrimitive.content })
            },
        )
    }

    private fun check(response: String): JsonObject {
        val o = json.parseToJsonElement(response).jsonObject
        if (o["ok"]?.jsonPrimitive?.booleanOrNull != true) {
            val msg = (o["errors"] as? JsonArray)?.joinToString("; ") {
                val e = it.jsonObject
                "${e.str("path")} ${e.str("message")}".trim()
            } ?: response
            throw EngineError(msg)
        }
        return o
    }

    /** Simule un traitement : concentrations, effets E(t), état de volume et états physiologiques. */
    fun treatment(inp: DemoInputs, plan: TreatmentPlan): TreatmentResult {
        require(plan.regimens.isNotEmpty()) { "aucun schéma d'administration" }
        val p = check(engine.newPatient("""{"bw": ${inp.bw}, "seed": ${inp.seed}, "variability": false}"""))
        val id = p.str("patient_id")
        if (inp.diseaseId != null) {
            val stage = inp.stage?.let { ""","stage": "$it"""" } ?: ""
            check(engine.addDisease(id, """{"id": "${inp.diseaseId}"$stage}"""))
        }
        val h = plan.horizonH
        val step = when { h <= 48 -> 0.1; h <= 7 * 24 -> 0.25; else -> 0.5 }
        val n = kotlin.math.floor(h / step).toInt()
        val grid = (0..n).joinToString(", ") { "%.4f".fmt(it * step).replace(',', '.') }
        val sStep = kotlin.math.max(1.0, kotlin.math.ceil(h / 96.0))
        val sn = kotlin.math.floor(h / sStep).toInt()
        val sgrid = (0..sn).joinToString(", ") { "%.4f".fmt(it * sStep).replace(',', '.') }
        val r = check(engine.timecourse(id, """{"doses": ${plan.dosesJson()}, "t_end_h": $h, "t_eval_h": [$grid], "state_times_h": [$sgrid]}"""))
        val series = { o: JsonObject -> o.mapValues { it.value.floats() } }
        val states = r["states_at"]!!.jsonArray.map { it.jsonObject }
        // toutes les variables numériques du modèle (clés de `eval`), pour choisir les indicateurs ;
        // les entrées « décès » n'ont pas de variables : NaN
        val num = { o: JsonObject, k: String -> (o[k] as? JsonPrimitive)?.takeIf { it.booleanOrNull == null && !it.isString }?.doubleOrNull }
        val keys = states.flatMap { o -> o.keys.filter { num(o, it) != null } }.distinct()
        val outcomes = states.map { it["outcome"]?.jsonPrimitive?.contentOrNull ?: "stable" }
        val firstDead = outcomes.indexOf("dead")
        val stateT = r["state_t"]!!.jsonArray.map { it.d() }.toDoubleArray()
        val death = if (firstDead >= 0) {
            val c = states[firstDead]["cause"]?.jsonPrimitive?.contentOrNull ?: ""
            Death(stateT[firstDead], c, DEATH_FR[c] ?: "décès")
        } else null
        return TreatmentResult(
            t = r["t"]!!.jsonArray.map { it.d() }.toDoubleArray(),
            conc = series(r["conc_mg_L"]!!.jsonObject),
            effect = series(r["effect"]!!.jsonObject),
            deficitMl = r["D"]!!.floats(),
            diuresisMlKg = r["U"]!!.floats(),
            stateT = stateT,
            states = keys.associateWith { k -> FloatArray(states.size) { i -> num(states[i], k)?.toFloat() ?: Float.NaN } },
            outcomes = outcomes,
            mechanisms = states.map { o -> (o["mechanisms"] as? JsonArray)?.map { it.jsonPrimitive.content } ?: emptyList() },
            death = death,
            plan = plan,
        )
    }

    /** Auscultation du patient décrit par [inp] (FC imposée si [hr] non nul, site d'auscultation). */
    @OptIn(kotlin.io.encoding.ExperimentalEncodingApi::class)
    fun auscultate(inp: DemoInputs, hr: Double?, site: String, durationS: Double = 8.0): AuscultationResult {
        val p = check(engine.newPatient("""{"bw": ${inp.bw}, "seed": ${inp.seed}, "variability": false}"""))
        val id = p.str("patient_id")
        if (inp.diseaseId != null) {
            val stage = inp.stage?.let { ""","stage": "$it"""" } ?: ""
            check(engine.addDisease(id, """{"id": "${inp.diseaseId}"$stage}"""))
        }
        val hrOpt = hr?.let { ""","hr": $it""" } ?: ""
        val r = check(engine.pcg(id, """{${inp.exposureJson()}, "site": "$site", "seed": ${inp.seed}, "duration_s": $durationS,
            "with_ecg": true, "with_pressures": true$hrOpt}"""))
        val wav = kotlin.io.encoding.Base64.decode(r.str("wav_base64"))
        val n = (wav.size - 44) / 2
        val samples = FloatArray(n) { i ->
            val lo = wav[44 + 2 * i].toInt() and 0xFF
            val hi = wav[45 + 2 * i].toInt()
            ((hi shl 8) or lo).toShort() / 32767f
        }
        val pr = r["pressures"]!!.jsonObject
        return AuscultationResult(
            samples = samples,
            fs = r["fs"]!!.d().toInt(),
            durationS = r["duration_s"]!!.d(),
            grade = r["grade"]?.jsonPrimitive?.intOrNull ?: 0,
            murmur = r.str("murmur"),
            s3 = r["s3"]?.jsonPrimitive?.booleanOrNull == true,
            hr = r["hr"]!!.d(),
            site = r.str("site"),
            beats = r["beats"]!!.jsonArray.map { it.d() }.toDoubleArray(),
            ecg = r["ecg"]!!.floats(),
            ecgFs = r["ecg_fs"]!!.d(),
            pressureFs = pr["fs"]!!.d(),
            ao = pr["ao"]!!.floats(),
            lv = pr["lv"]!!.floats(),
            la = pr["la"]!!.floats(),
        )
    }

    /** Évalue un patient déterministe (sans variabilité) avec maladie et concentrations. */
    fun evaluate(inp: DemoInputs, ecgSeconds: Double = 8.0): DemoResult {
        val p = check(engine.newPatient("""{"bw": ${inp.bw}, "seed": ${inp.seed}, "variability": false}"""))
        val id = p.str("patient_id")
        var caseLabel = "sain"
        if (inp.diseaseId != null) {
            val stage = inp.stage?.let { ""","stage": "$it"""" } ?: ""
            val c = check(engine.addDisease(id, """{"id": "${inp.diseaseId}"$stage}"""))["case"]!!.jsonObject
            val name = catalog.diseases.firstOrNull { it.id == inp.diseaseId }?.name ?: inp.diseaseId
            caseLabel = "$name, stade ${c.str("stage")} (s = ${"%.2f".fmt(c["severity"]!!.d())})"
        }
        val st = check(engine.state(id, """{${inp.exposureJson()}, "signals": true, "duration_s": $ecgSeconds, "seed": ${inp.seed}}"""))
        val o = st["outcome"]!!.jsonObject
        // Décès absorbant : `state` est instantané ; un décès daté avant l'instant examiné persiste.
        val outcome = if (inp.deadAtExam) {
            Outcome("dead", cause = inp.death!!.cause, label = inp.death.label, sinceH = inp.death.tH)
        } else {
            Outcome(
                state = o.str("state"),
                mechanisms = (o["mechanisms"] as? JsonArray)?.map {
                    val m = it.jsonObject
                    Mechanism(m.str("id"), m.str("label_fr"), m["value"]!!.d(), m["threshold"]!!.d(), m["factor"]!!.d())
                } ?: emptyList(),
                cause = o["cause"]?.jsonPrimitive?.contentOrNull,
                label = o["label_fr"]?.jsonPrimitive?.contentOrNull,
            )
        }
        val values = if (inp.deadAtExam) emptyMap() else st.filterValues { it is JsonPrimitive && it.jsonPrimitive.doubleOrNull != null && it.jsonPrimitive.booleanOrNull == null }
            .mapValues { it.value.d() }
        val signs = if (outcome.dead) emptyList() else st["emergent_signs"]!!.jsonArray.map {
            val so = it.jsonObject
            Sign(so.str("label_fr"), so.str("source"))
        }
        val sg = if (outcome.dead) null else st["signals"] as? JsonObject
        val pf = sg?.get("pcg_features")?.jsonObject
        val grade = pf?.get("grade")?.jsonPrimitive?.intOrNull ?: 0
        val murmur = pf?.str("murmur") ?: ""
        val pr = sg?.get("pressures")?.jsonObject
        val rs = sg?.get("respiration")?.jsonObject
        val pal = sg?.get("pressures_aligned") as? JsonObject
        val empty = FloatArray(0)
        // Paramètres du patient comparés à ceux du chien sain de même poids.
        val hp = check(engine.newPatient("""{"bw": ${inp.bw}, "variability": false}""")).str("patient_id")
        val healthy = check(engine.state(hp, "{}"))["params"]!!.jsonObject
        val current = st["params"]!!.jsonObject
        val disease = catalog.diseases.firstOrNull { it.id == inp.diseaseId }
        val drugIds = if (inp.plan != null && inp.atH != null) inp.plan.regimens.map { it.drugId }.toSet() else emptySet()
        val drugs = catalog.drugs.filter { it.id in drugIds }
        val changes = current.keys.mapNotNull { k ->
            val a = healthy[k]!!.d()
            val b = current[k]!!.d()
            if (a == b || kotlin.math.abs(b - a) <= 1e-12 * kotlin.math.max(kotlin.math.abs(a), kotlin.math.abs(b))) return@mapNotNull null
            val sources = listOfNotNull(disease?.takeIf { d -> d.effects.any { it.param == k } }?.name) +
                drugs.filter { d -> d.effects.any { it.param == k } }.map { it.name }
            ParamChange(k, a, b, catalog.blocks.filter { k in it.params }.map { it.id }, sources)
        }
        return DemoResult(
            values = values,
            caseLabel = caseLabel + when {
                inp.plan != null && inp.atH != null -> " · sous traitement, examiné à ${dayLabel(inp.atH)} (t = ${"%.1f".fmt(inp.atH)} h)"
                else -> ""
            },
            signs = signs,
            ecg = sg?.get("ecg")?.floats() ?: empty,
            ecgFs = sg?.get("ecg_fs")?.d() ?: 500.0,
            pcg = sg?.get("pcg")?.floats() ?: empty,
            pcgFs = sg?.get("pcg_fs")?.d()?.toInt() ?: 4000,
            pcgLabel = if (grade == 0) "pas de souffle" else "souffle $murmur, grade $grade/6",
            pressureDt = pr?.get("dt")?.d() ?: 0.002,
            ao = pr?.get("ao")?.floats() ?: empty,
            lv = pr?.get("lv")?.floats() ?: empty,
            la = pr?.get("la")?.floats() ?: empty,
            alignedFs = pal?.get("fs")?.d() ?: 250.0,
            alignedAo = pal?.get("ao")?.floats() ?: empty,
            alignedLv = pal?.get("lv")?.floats() ?: empty,
            alignedLa = pal?.get("la")?.floats() ?: empty,
            respiration = if (rs == null) {
                Respiration(fs = 1.0, rrPerMin = 0.0, vtAlvMl = 0.0, volume = empty, flow = empty, hr = empty, map = empty, sap = empty, dap = empty)
            } else {
                Respiration(
                    fs = rs["fs"]!!.d(), rrPerMin = rs["rr_per_min"]!!.d(), vtAlvMl = rs["vt_alv_mL"]!!.d(),
                    volume = rs["volume_mL"]!!.floats(), flow = rs["flow_mL_s"]!!.floats(),
                    hr = rs["hr"]!!.floats(), map = rs["map"]!!.floats(),
                    sap = rs["sap"]!!.floats(), dap = rs["dap"]!!.floats(),
                )
            },
            changes = changes,
            diseaseId = inp.diseaseId,
            drugIds = drugs.map { it.id },
            outcome = outcome,
        )
    }
}

/** Formatage indépendant de la plateforme (pas de String.format en Kotlin commun) :
 * `"%.2f".fmt(x)` → arrondi à 2 décimales, virgule décimale. Arithmétique entière : pas de
 * notation scientifique, même pour les petites valeurs. */
fun String.fmt(x: Double): String {
    val decimals = substringAfter('.').substringBefore('f').toIntOrNull() ?: 2
    if (x.isNaN() || x.isInfinite()) return "—"
    var f = 1L
    repeat(decimals) { f *= 10 }
    val scaled = kotlin.math.round(kotlin.math.abs(x) * f).toLong()
    val sign = if (x < 0 && scaled != 0L) "-" else ""
    val int = (scaled / f).toString()
    return if (decimals == 0) sign + int else sign + int + "," + (scaled % f).toString().padStart(decimals, '0')
}

/** Spectrogramme par transformée de Fourier à court terme (fenêtre de Hann). */
class Spectrogram(val frames: Int, val bins: Int, val db: FloatArray, val hopS: Double, val binHz: Double, val minDb: Float, val maxDb: Float) {
    operator fun get(frame: Int, bin: Int): Float = db[frame * bins + bin]

    companion object {
        fun of(x: FloatArray, fs: Int, nfft: Int, hop: Int, maxHz: Double): Spectrogram {
            val binHz = fs.toDouble() / nfft
            val bins = minOf(nfft / 2, (maxHz / binHz).toInt() + 1)
            val frames = maxOf(0, (x.size - nfft) / hop + 1)
            val win = DoubleArray(nfft) { 0.5 - 0.5 * kotlin.math.cos(2 * kotlin.math.PI * it / nfft) }
            val out = FloatArray(frames * bins)
            val re = DoubleArray(nfft)
            val im = DoubleArray(nfft)
            for (f in 0 until frames) {
                for (i in 0 until nfft) { re[i] = x[f * hop + i] * win[i]; im[i] = 0.0 }
                fft(re, im)
                for (b in 0 until bins) {
                    val p = re[b] * re[b] + im[b] * im[b]
                    out[f * bins + b] = (10 * kotlin.math.log10(p + 1e-12)).toFloat()
                }
            }
            val maxDb = out.maxOrNull() ?: 0f
            return Spectrogram(frames, bins, out, hop.toDouble() / fs, binHz, maxDb - 60f, maxDb)
        }

        /** FFT radix 2 en place (n puissance de 2). */
        private fun fft(re: DoubleArray, im: DoubleArray) {
            val n = re.size
            var j = 0
            for (i in 1 until n) {
                var bit = n shr 1
                while (j and bit != 0) { j = j xor bit; bit = bit shr 1 }
                j = j xor bit
                if (i < j) { val tr = re[i]; re[i] = re[j]; re[j] = tr; val ti = im[i]; im[i] = im[j]; im[j] = ti }
            }
            var len = 2
            while (len <= n) {
                val ang = -2 * kotlin.math.PI / len
                for (i in 0 until n step len) {
                    for (k in 0 until len / 2) {
                        val c = kotlin.math.cos(ang * k); val s = kotlin.math.sin(ang * k)
                        val ur = re[i + k]; val ui = im[i + k]
                        val vr = re[i + k + len / 2] * c - im[i + k + len / 2] * s
                        val vi = re[i + k + len / 2] * s + im[i + k + len / 2] * c
                        re[i + k] = ur + vr; im[i + k] = ui + vi
                        re[i + k + len / 2] = ur - vr; im[i + k + len / 2] = ui - vi
                    }
                }
                len = len shl 1
            }
        }
    }
}
