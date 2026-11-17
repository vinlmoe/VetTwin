package fr.vetsim.demo

import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.unit.Density
import java.io.File
import org.jetbrains.skia.EncodedImageFormat
import kotlin.test.Test
import kotlin.test.assertTrue

/** Rendu hors écran de la démo (critère M7) : MMVD stade C, ECG, pressions, gaz du sang. */
class ScreenshotTest {
    @Test
    fun renderMmvdStageC() {
        val controller = DemoController()
        val scene = ImageComposeScene(width = 1400, height = 1180, density = Density(1f)) {
            App(controller, initial = DemoInputs(diseaseId = "mmvd", stage = "C"), autoEvaluate = true)
        }
        scene.render(0)            // composition + LaunchedEffect
        val img = scene.render(1_000_000_000L)
        val out = File(System.getProperty("vetsim.screenshots")).apply { mkdirs() }.resolve("mmvd_stage_C.png")
        out.writeBytes(img.encodeToData(EncodedImageFormat.PNG)!!.bytes)
        scene.close()
        assertTrue(out.length() > 10_000)
        println("capture : ${out.path}")
    }
}
