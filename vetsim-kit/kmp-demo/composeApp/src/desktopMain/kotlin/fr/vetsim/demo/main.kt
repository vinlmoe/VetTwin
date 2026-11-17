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

@Volatile
private var currentLine: javax.sound.sampled.SourceDataLine? = null

actual fun playAudio(samples: FloatArray, sampleRate: Int) {
    stopAudio()
    val pcm = ByteArray(samples.size * 2)
    samples.forEachIndexed { i, s ->
        val v = (s.coerceIn(-1f, 1f) * 0.9f * Short.MAX_VALUE).toInt()
        pcm[2 * i] = (v and 0xFF).toByte()
        pcm[2 * i + 1] = ((v shr 8) and 0xFF).toByte()
    }
    val fmt = AudioFormat(sampleRate.toFloat(), 16, 1, true, false)
    val line = AudioSystem.getSourceDataLine(fmt)
    currentLine = line
    thread(isDaemon = true) {
        try {
            line.open(fmt); line.start(); line.write(pcm, 0, pcm.size); line.drain()
        } catch (_: Exception) {
            // ligne fermée par stopAudio
        } finally {
            line.close()
            if (currentLine === line) currentLine = null
        }
    }
}

actual fun stopAudio() {
    currentLine?.let { it.stop(); it.flush(); it.close() }
    currentLine = null
}
