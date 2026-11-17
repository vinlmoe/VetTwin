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
)

data class Sign(val label: String, val source: String)

data class DemoInputs(
    val bw: Double = 20.0,
    val diseaseId: String? = null,
    val stage: String? = null,
    val conc: Map<String, Double> = emptyMap(),
    val seed: Long = 1,
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
        val conc = inp.conc.entries.joinToString(", ") { (k, v) -> "\"$k\": $v" }
        val st = check(engine.state(id, """{"conc": {$conc}, "signals": true, "duration_s": $ecgSeconds, "seed": ${inp.seed}}"""))
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
        val drugs = catalog.drugs.filter { it.id in inp.conc.keys }
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
            caseLabel = caseLabel + if (inp.conc.isEmpty()) "" else " + " + inp.conc.entries.joinToString { "${it.key} ${it.value} mg/L" },
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
