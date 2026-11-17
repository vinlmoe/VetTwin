package fr.vetsim.demo

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
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
) {
    fun doseTimes(): List<Double> = List(count) { startH + it * intervalH }
}

/** Traitement : un ou plusieurs schémas, simulés sur [horizonH] heures. */
data class TreatmentPlan(val regimens: List<Regimen>, val horizonH: Double) {
    fun dosesJson(): String = regimens.flatMap { r ->
        r.doseTimes().map { t -> """{"t_h": $t, "drug": "${r.drugId}", "mg_per_kg": ${r.mgPerKg}, "route": "${r.route}"}""" }
    }.joinToString(", ", "[", "]")
}

data class DemoInputs(
    val bw: Double = 20.0,
    val diseaseId: String? = null,
    val stage: String? = null,
    val conc: Map<String, Double> = emptyMap(),
    val seed: Long = 1,
    /** Traitement en cours et instant d'examen (h) : remplacent `conc` quand ils sont définis. */
    val plan: TreatmentPlan? = null,
    val atH: Double? = null,
) {
    /** Options d'exposition au médicament pour state/pcg : schéma à l'instant t, ou concentrations fixes. */
    fun exposureJson(): String = if (plan != null && atH != null && plan.regimens.isNotEmpty()) {
        """"doses": ${plan.dosesJson()}, "at_h": $atH"""
    } else {
        """"conc": {${conc.entries.joinToString(", ") { (k, v) -> "\"$k\": $v" }}}"""
    }
}

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
    val states: Map<String, FloatArray>,
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
    val respiration: Respiration,
    val changes: List<ParamChange>,
    val diseaseId: String?,
    val drugIds: List<String>,
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
        // toutes les variables numériques du modèle (clés de `eval`), pour choisir les indicateurs
        val keys = states.firstOrNull()?.keys?.filter { states[0][it] is JsonPrimitive && states[0][it]!!.jsonPrimitive.doubleOrNull != null } ?: emptyList()
        return TreatmentResult(
            t = r["t"]!!.jsonArray.map { it.d() }.toDoubleArray(),
            conc = series(r["conc_mg_L"]!!.jsonObject),
            effect = series(r["effect"]!!.jsonObject),
            deficitMl = r["D"]!!.floats(),
            diuresisMlKg = r["U"]!!.floats(),
            stateT = r["state_t"]!!.jsonArray.map { it.d() }.toDoubleArray(),
            states = keys.associateWith { k -> FloatArray(states.size) { i -> states[i][k]!!.d().toFloat() } },
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
        val values = st.filterValues { it is JsonPrimitive && it.jsonPrimitive.doubleOrNull != null && it.jsonPrimitive.booleanOrNull == null }
            .mapValues { it.value.d() }
        val signs = st["emergent_signs"]!!.jsonArray.map {
            val o = it.jsonObject
            Sign(o.str("label_fr"), o.str("source"))
        }
        val sg = st["signals"]!!.jsonObject
        val pf = sg["pcg_features"]!!.jsonObject
        val grade = pf["grade"]?.jsonPrimitive?.intOrNull ?: 0
        val murmur = pf.str("murmur")
        val pr = sg["pressures"]!!.jsonObject
        val rs = sg["respiration"]!!.jsonObject
        // Paramètres du patient comparés à ceux du chien sain de même poids.
        val hp = check(engine.newPatient("""{"bw": ${inp.bw}, "variability": false}""")).str("patient_id")
        val healthy = check(engine.state(hp, "{}"))["params"]!!.jsonObject
        val current = st["params"]!!.jsonObject
        val disease = catalog.diseases.firstOrNull { it.id == inp.diseaseId }
        val drugIds = if (inp.plan != null && inp.atH != null) inp.plan.regimens.map { it.drugId }.toSet() else inp.conc.keys
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
                inp.conc.isEmpty() -> ""
                else -> " + " + inp.conc.entries.joinToString { "${it.key} ${it.value} mg/L" }
            },
            signs = signs,
            ecg = sg["ecg"]!!.floats(),
            ecgFs = sg["ecg_fs"]!!.d(),
            pcg = sg["pcg"]!!.floats(),
            pcgFs = sg["pcg_fs"]!!.d().toInt(),
            pcgLabel = if (grade == 0) "pas de souffle" else "souffle $murmur, grade $grade/6",
            pressureDt = pr["dt"]!!.d(),
            ao = pr["ao"]!!.floats(),
            lv = pr["lv"]!!.floats(),
            la = pr["la"]!!.floats(),
            respiration = Respiration(
                fs = rs["fs"]!!.d(), rrPerMin = rs["rr_per_min"]!!.d(), vtAlvMl = rs["vt_alv_mL"]!!.d(),
                volume = rs["volume_mL"]!!.floats(), flow = rs["flow_mL_s"]!!.floats(),
                hr = rs["hr"]!!.floats(), map = rs["map"]!!.floats(),
                sap = rs["sap"]!!.floats(), dap = rs["dap"]!!.floats(),
            ),
            changes = changes,
            diseaseId = inp.diseaseId,
            drugIds = drugs.map { it.id },
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
