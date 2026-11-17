package fr.vetsim.demo

import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.unit.Density
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
    fun renderModelDiagram() = capture(
        "modele_mmvd_C_pimobendane",
        DemoInputs(diseaseId = "mmvd", stage = "C", conc = mapOf("pimobendan" to 0.03)),
        tab = 2, block = "left_heart", height = 1500,
    )
}
