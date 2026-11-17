package fr.vetsim.demo

import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import javax.sound.sampled.AudioFormat
import javax.sound.sampled.AudioSystem
import kotlin.concurrent.thread

fun main() = application {
    val controller = DemoController()
    Window(
        onCloseRequest = ::exitApplication,
        title = "VetTwins : patient virtuel canin (démo pédagogique)",
        state = rememberWindowState(width = 1400.dp, height = 920.dp),
    ) {
        App(controller)
    }
}

actual fun playAudio(samples: FloatArray, sampleRate: Int) {
    thread(isDaemon = true) {
        val pcm = ByteArray(samples.size * 2)
        samples.forEachIndexed { i, s ->
            val v = (s.coerceIn(-1f, 1f) * 0.9f * Short.MAX_VALUE).toInt()
            pcm[2 * i] = (v and 0xFF).toByte()
            pcm[2 * i + 1] = ((v shr 8) and 0xFF).toByte()
        }
        val fmt = AudioFormat(sampleRate.toFloat(), 16, 1, true, false)
        AudioSystem.getSourceDataLine(fmt).apply {
            open(fmt); start(); write(pcm, 0, pcm.size); drain(); close()
        }
    }
}
