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
data class DiseaseInfo(val id: String, val name: String, val evidence: String, val stages: List<StageInfo>)
data class DrugInfo(val id: String, val name: String, val evidence: String, val ec50: Double)
data class Catalog(val diseases: List<DiseaseInfo>, val drugs: List<DrugInfo>)

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
)

class EngineError(message: String) : Exception(message)

private fun JsonElement.d(): Double = jsonPrimitive.doubleOrNull ?: Double.NaN
private fun JsonElement.floats(): FloatArray = jsonArray.map { it.jsonPrimitive.doubleOrNull?.toFloat() ?: 0f }.toFloatArray()
private fun JsonObject.str(k: String): String = (this[k] as? JsonPrimitive)?.content ?: ""

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
                )
            },
            drugs = c["drugs"]!!.jsonArray.map { d ->
                val o = d.jsonObject
                DrugInfo(o.str("id"), o.str("name"), o.str("evidence"), o["ec50_mg_L"]!!.d())
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
        )
    }
}

/** Formatage indépendant de la plateforme (pas de String.format en Kotlin commun). */
fun String.fmt(x: Double): String {
    val decimals = substringAfter('.').substringBefore('f').toIntOrNull() ?: 2
    if (x.isNaN()) return "—"
    var f = 1.0
    repeat(decimals) { f *= 10 }
    val r = kotlin.math.round(x * f) / f
    val s = r.toString()
    val parts = s.split('.')
    val frac = if (parts.size > 1) parts[1] else ""
    return if (decimals == 0) parts[0] else parts[0] + "," + frac.padEnd(decimals, '0').take(decimals)
}
