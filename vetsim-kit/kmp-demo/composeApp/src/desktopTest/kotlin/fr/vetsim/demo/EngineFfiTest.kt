package fr.vetsim.demo

import java.io.File
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.double
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Moteur Rust appelé depuis la JVM par UniFFI/JNA. */
class EngineFfiTest {
    private val kit = File(System.getProperty("vetsim.kit"))

    @Test
    fun mmvdStageCThroughFfi() {
        val c = DemoController()
        assertEquals("1.1.0", c.apiVersion)
        assertEquals(8, c.catalog.diseases.size) // 4 maladies + 4 troubles du rythme
        val r = c.evaluate(DemoInputs(diseaseId = "mmvd", stage = "C"))
        assertEquals(4000, r.ecg.size)
        assertTrue(r.ao.isNotEmpty() && r.lv.size == r.ao.size)
        // stade C congestif : POG nettement au-dessus du stade B (7 mmHg) ; milieu du stade C
        // 13,9 mmHg depuis MMVD 1.2.0 (médiane d'Allen 2021 en ICC : 14, 8–32)
        assertTrue(r.values.getValue("la_mean") > 12.0, "POG ${r.values["la_mean"]}")
        assertTrue(r.signs.any { it.label.startsWith("souffle") }, "${r.signs}")
    }

    /** Même entrée, mêmes bits : JVM (FFI) contre le binaire natif `vetsim-cli eval`. */
    @Test
    fun ffiIsBitIdenticalToNativeCli() {
        val cli = kit.resolve("target/release/vetsim-cli")
        if (!cli.canExecute()) error("construire d'abord vetsim-cli : cargo build --release -p vetsim-cli")
        for ((disease, sev) in listOf("mmvd" to 0.8, "addison" to 0.9, "ckd" to 0.6)) {
            val proc = ProcessBuilder(cli.path, "eval", "--disease", "$disease:$sev").directory(kit).start()
            val native = Json.parseToJsonElement(proc.inputStream.bufferedReader().readText()).jsonObject
            check(proc.waitFor() == 0)
            val e = FfiEngine()
            e.loadModules(MODULES_BUNDLE)
            val id = Json.parseToJsonElement(e.newPatient("""{"variability": false}""")).jsonObject["patient_id"]!!.jsonPrimitive.content
            e.addDisease(id, """{"id": "$disease", "severity": $sev}""")
            val st = Json.parseToJsonElement(e.state(id, "{}")).jsonObject
            for ((k, v) in native) {
                val a = v.jsonPrimitive.double
                val b = st[k]!!.jsonPrimitive.double
                assertEquals(a.toRawBits(), b.toRawBits(), "$disease:$sev $k : natif $a / JVM $b")
            }
        }
    }
}
