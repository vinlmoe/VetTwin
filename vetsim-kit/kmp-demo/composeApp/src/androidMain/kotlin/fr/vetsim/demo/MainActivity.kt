package fr.vetsim.demo

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.runtime.remember

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            val controller = remember { DemoController() }
            App(controller)
        }
    }
}

private var currentTrack: AudioTrack? = null

actual fun stopAudio() {
    currentTrack?.let { runCatching { it.stop() }; it.release() }
    currentTrack = null
}

actual fun playAudio(samples: FloatArray, sampleRate: Int) {
    stopAudio()
    val pcm = ShortArray(samples.size) { (samples[it].coerceIn(-1f, 1f) * 0.9f * Short.MAX_VALUE).toInt().toShort() }
    val track = AudioTrack.Builder()
        .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build())
        .setAudioFormat(
            AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setSampleRate(sampleRate).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build(),
        )
        .setTransferMode(AudioTrack.MODE_STATIC)
        .setBufferSizeInBytes(pcm.size * 2)
        .build()
    track.write(pcm, 0, pcm.size)
    currentTrack = track
    track.play()
}
