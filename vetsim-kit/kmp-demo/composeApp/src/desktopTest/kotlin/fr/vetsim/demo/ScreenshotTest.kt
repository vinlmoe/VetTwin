package fr.vetsim.demo

import androidx.compose.ui.ImageComposeScene
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import java.io.File
import org.jetbrains.skia.EncodedImageFormat
import kotlin.test.Test
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
    fun renderModelDiagram() = capture(
        "modele_mmvd_C_pimobendane",
        DemoInputs(diseaseId = "mmvd", stage = "C", conc = mapOf("pimobendan" to 0.03)),
        tab = 2, block = "left_heart", height = 1500,
    )
}
