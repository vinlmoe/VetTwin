package fr.vetsim.demo

import androidx.compose.ui.ImageComposeScene
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import java.io.File
import org.jetbrains.skia.EncodedImageFormat
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/** Rendu hors écran de la démo (critère M7) : MMVD stade C, ECG, pressions, gaz du sang. */
class ScreenshotTest {
    private fun capture(name: String, inputs: DemoInputs, tab: Int, block: String? = null, height: Int = 1180) {
        val controller = DemoController()
        val scene = ImageComposeScene(width = 1400, height = height, density = Density(1f)) {
            App(controller, initial = inputs, autoEvaluate = true, initialTab = tab, initialBlock = block)
        }
        scene.render(0)            // composition + LaunchedEffect
        val img = scene.render(1_000_000_000L)
        val out = File(System.getProperty("vetsim.screenshots")).apply { mkdirs() }.resolve("$name.png")
        out.writeBytes(img.encodeToData(EncodedImageFormat.PNG)!!.bytes)
        scene.close()
        assertTrue(out.length() > 10_000)
        println("capture : ${out.path}")
    }

    @Test
    fun renderMmvdStageC() = capture("mmvd_stage_C", DemoInputs(diseaseId = "mmvd", stage = "C"), tab = 0)

    @Test
    fun renderRespiration() = capture("respiration_mmvd_C", DemoInputs(diseaseId = "mmvd", stage = "C"), tab = 1)

    @Test
    fun renderAuscultation() {
        val controller = DemoController()
        val inputs = DemoInputs(diseaseId = "mmvd", stage = "B2")
        val scene = ImageComposeScene(width = 1100, height = 900, density = Density(1f)) {
            androidx.compose.foundation.layout.Box(
                androidx.compose.ui.Modifier.background(androidx.compose.ui.graphics.Color(0xFFF6F7F8)).padding(16.dp),
            ) {
                androidx.compose.material3.MaterialTheme {
                    AuscultationView(controller, inputs, initialWindowS = 2f, initialCursor = 1.2f)
                }
            }
        }
        scene.render(0)
        val img = scene.render(1_000_000_000L)
        val out = File(System.getProperty("vetsim.screenshots")).apply { mkdirs() }.resolve("auscultation_mmvd_B2.png")
        out.writeBytes(img.encodeToData(EncodedImageFormat.PNG)!!.bytes)
        scene.close()
        assertTrue(out.length() > 10_000)
        // Mêmes battements pour le son et l'ECG
        val r = controller.auscultate(inputs, null, "base")
        assertTrue(r.samples.size == 32000 && r.ecg.size == 4000 && r.ao.size == 2000)
        assertTrue(r.grade >= 2, "grade ${r.grade}")
    }

    @Test
    fun renderTreatment() {
        val controller = DemoController()
        val inputs = DemoInputs(diseaseId = "mmvd", stage = "C")
        val plan = TreatmentPlan(
            listOf(
                Regimen("furosemide", 2.0, "po", 12.0, 14),
                Regimen("pimobendan", 0.25, "po", 12.0, 14),
            ),
            horizonH = 7 * 24.0,
        )
        val scene = ImageComposeScene(width = 1250, height = 2050, density = Density(1f)) {
            androidx.compose.foundation.layout.Box(
                androidx.compose.ui.Modifier.background(androidx.compose.ui.graphics.Color(0xFFF6F7F8)).padding(16.dp),
            ) {
                androidx.compose.material3.MaterialTheme {
                    TreatmentView(controller, inputs, onExamine = { _, _, _ -> }, initialPlan = plan, initialCursorH = 61.0, autoSimulate = true)
                }
            }
        }
        scene.render(0)
        val img = scene.render(1_000_000_000L)
        val out = File(System.getProperty("vetsim.screenshots")).apply { mkdirs() }.resolve("traitement_mmvd_C.png")
        out.writeBytes(img.encodeToData(EncodedImageFormat.PNG)!!.bytes)
        scene.close()
        assertTrue(out.length() > 10_000)
        // Examen à t = 61 h : la vue Patient et l'auscultation utilisent le schéma
        val exam = inputs.copy(plan = plan, atH = 61.0)
        val r = controller.evaluate(exam)
        assertTrue(r.caseLabel.contains("J2 13 h"), r.caseLabel)
        assertTrue(r.changes.any { it.param == "vol_shift" }, "effet du diurétique (D) attendu")
        assertTrue(controller.auscultate(exam, null, "base").samples.size == 32000)
        // Prise omise et prise doublée : la cinétique en tient compte
        val pim = plan.regimens[1]
        val edited = plan.copy(regimens = listOf(plan.regimens[0], pim.copy(overrides = mapOf(4 to 0.0, 6 to 0.5))))
        val base = controller.treatment(inputs, plan)
        val ed = controller.treatment(inputs, edited)
        val c0 = base.conc.getValue("pimobendan")
        val c1 = ed.conc.getValue("pimobendan")
        fun at(r: TreatmentResult, h: Double) = r.t.indices.minBy { kotlin.math.abs(r.t[it] - h) }
        assertTrue(c1[at(ed, 49.0)] < 0.05f * c0[at(base, 49.0)], "prise de 48 h omise")
        assertTrue(c1[at(ed, 73.0)] > 1.8f * c0[at(base, 73.0)], "prise de 72 h doublée")
        assertTrue(edited.dosesJson().split("pimobendan").size - 1 == 13, "13 prises effectives")
    }

    @Test
    fun renderOverdoseDeath() {
        val c = DemoController()
        val inputs = DemoInputs()
        val plan = TreatmentPlan(listOf(Regimen("opioide", 20.0, "iv", 24.0, 1, startH = 2.0)), horizonH = 24.0)
        val r = c.treatment(inputs, plan)
        val death = assertNotNull(r.death, "surdosage d'opioïde : décès attendu")
        assertEquals("oxygen_debt", death.cause)
        // décès absorbant : plus aucun instant « vivant » après le décès
        val i = r.stateT.indexOfFirst { it >= death.tH }
        assertTrue(r.outcomes.drop(i).all { it == "dead" })
        assertTrue(r.states.getValue("map").drop(i).all { it.isNaN() })
        // examen postérieur au décès : le patient reste décédé
        val ex = c.evaluate(inputs.copy(plan = plan, atH = 12.0, death = death))
        assertTrue(ex.outcome.dead)
        assertTrue(ex.ecg.isEmpty())
        capture("deces_opioide", inputs.copy(plan = plan, atH = 12.0, death = death), tab = 0)
    }

    @Test
    fun renderModelDiagram() = capture(
        "modele_mmvd_C_pimobendane",
        DemoInputs(
            diseaseId = "mmvd", stage = "C",
            plan = TreatmentPlan(listOf(Regimen("pimobendan", 0.25, "po", 12.0, 2)), horizonH = 24.0), atH = 2.0,
        ),
        tab = 2, block = "left_heart", height = 1500,
    )
}
