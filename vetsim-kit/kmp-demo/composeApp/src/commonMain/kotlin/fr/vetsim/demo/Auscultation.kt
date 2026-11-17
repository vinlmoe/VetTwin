package fr.vetsim.demo

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.floor
import kotlin.time.TimeSource

private val Ink = Color(0xFF1B2A35)
private val Muted = Color(0xFF5B6B77)
private val Accent = Color(0xFF0B5E7A)
private val CursorColor = Color(0xFFE0465A)
private val AoRed = Color(0xFFC0392B)
private val LvBlue = Color(0xFF2563A8)
private val LaAmber = Color(0xFFB7791F)
private val EcgGreen = Color(0xFF0B7A55)

/**
 * Auscultation : son synthétisé (module pcg.rs), forme d'onde, spectrogramme, ECG et pressions
 * cardiaques sur le même axe de temps ; zoom et défilement ; curseur synchronisé avec le son.
 */
@Composable
fun AuscultationView(
    controller: DemoController,
    inputs: DemoInputs,
    initial: AuscultationResult? = null,
    initialWindowS: Float? = null,
    initialCursor: Float? = null,
) {
    var hrChoice by remember { mutableStateOf<Double?>(null) }
    var site by remember { mutableStateOf("base") }
    var result by remember { mutableStateOf(initial) }
    var error by remember { mutableStateOf<String?>(null) }
    var window by remember { mutableStateOf(initialWindowS ?: 8f) }
    var start by remember { mutableStateOf(0f) }
    var cursor by remember { mutableStateOf(initialCursor) }
    var playing by remember { mutableStateOf(false) }

    fun generate() {
        try {
            error = null
            result = controller.auscultate(inputs, hrChoice, site)
        } catch (e: Exception) {
            error = e.message
        }
    }
    if (result == null && error == null) LaunchedEffect(inputs) { generate() }

    var loop by remember { mutableStateOf(false) }
    var zoomOnly by remember { mutableStateOf(false) }
    var played by remember { mutableStateOf(false) }
    var gain by remember { mutableStateOf(3f) }

    /** Amplification réglable avec limiteur doux (tanh) : pas d'écrêtage brutal. */
    fun amplified(x: FloatArray): FloatArray = FloatArray(x.size) { kotlin.math.tanh(gain * x[it]) }
    // segment joué (s) : tout l'enregistrement, ou la fenêtre affichée
    var segStart by remember { mutableStateOf(0f) }
    var segLen by remember { mutableStateOf(0f) }
    var playId by remember { mutableStateOf(0) }

    fun startPlayback() {
        val r = result ?: return
        val dur = r.durationS.toFloat()
        segStart = if (zoomOnly) start else 0f
        segLen = if (zoomOnly) minOf(window, dur - segStart) else dur
        val a = (segStart * r.fs).toInt().coerceIn(0, r.samples.size)
        val b = ((segStart + segLen) * r.fs).toInt().coerceIn(a, r.samples.size)
        playAudio(amplified(r.samples.copyOfRange(a, b)), r.fs)
        if (!zoomOnly) start = 0f
        played = true
        playing = true
        playId++
    }
    fun stopPlayback() { stopAudio(); playing = false }
    DisposableEffect(Unit) { onDispose { stopAudio() } }

    // Curseur synchronisé : temps écoulé depuis le lancement du son ; la fenêtre suit le curseur ;
    // en boucle, le segment est relancé à la fin.
    LaunchedEffect(playing, playId) {
        if (!playing) return@LaunchedEffect
        var mark = TimeSource.Monotonic.markNow()
        val dur = result?.durationS?.toFloat() ?: 0f
        while (playing) {
            withFrameNanos { }
            val el = mark.elapsedNow().inWholeMilliseconds / 1000f
            val t = segStart + minOf(el, segLen)
            cursor = t
            if (!zoomOnly && t > start + 0.9f * window) start = (t - 0.1f * window).coerceIn(0f, maxOf(0f, dur - window))
            if (el >= segLen) {
                if (loop) {
                    val r = result ?: break
                    val a = (segStart * r.fs).toInt().coerceIn(0, r.samples.size)
                    val b = ((segStart + segLen) * r.fs).toInt().coerceIn(a, r.samples.size)
                    playAudio(amplified(r.samples.copyOfRange(a, b)), r.fs)
                    if (!zoomOnly) start = 0f
                    mark = TimeSource.Monotonic.markNow()
                } else {
                    playing = false
                }
            }
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("FC :", color = Muted, fontSize = 13.sp)
            Chip("Patient", hrChoice == null) { stopPlayback(); played = false; hrChoice = null; generate() }
            Chip("Repos 72", hrChoice == 72.0) { stopPlayback(); played = false; hrChoice = 72.0; generate() }
            Chip("90", hrChoice == 90.0) { stopPlayback(); played = false; hrChoice = 90.0; generate() }
            Text("   Site :", color = Muted, fontSize = 13.sp)
            Chip("Base", site == "base") { stopPlayback(); played = false; site = "base"; generate() }
            Chip("Apex (non calibré)", site == "apex") { stopPlayback(); played = false; site = "apex"; generate() }
        }
        val r = result
        error?.let { Text("Erreur : $it", color = Color(0xFFB00020)) }
        if (r == null) return@Column
        val dur = r.durationS.toFloat()
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(onClick = { if (playing) stopPlayback() else startPlayback() }) {
                Text(if (playing) "■ Arrêter" else if (played) "↻ Rejouer" else "▶ Écouter")
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(loop, { loop = it })
                Text("en boucle", color = Ink, fontSize = 13.sp)
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(zoomOnly, { zoomOnly = it })
                Text("fenêtre affichée seulement", color = Ink, fontSize = 13.sp)
            }
            Text("Volume ×${"%.1f".fmt(gain.toDouble())}", color = Ink, fontSize = 13.sp)
            Slider(value = gain, onValueChange = { gain = it }, valueRange = 1f..8f, modifier = Modifier.width(140.dp))
            Text(
                "FC ${"%.0f".fmt(r.hr)} /min · ${r.beats.size} battements · " +
                    (if (r.grade == 0) "pas de souffle" else "souffle ${r.murmur}, grade ${r.grade}/6 (indicatif)") +
                    (if (r.s3) " · B3" else "") + " · site ${r.site}",
                color = Ink, fontSize = 13.sp,
            )
        }
        // Zoom et défilement
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Fenêtre : ${"%.1f".fmt(window.toDouble())} s", color = Muted, fontSize = 12.sp)
                Slider(value = window, onValueChange = {
                    window = it
                    start = start.coerceIn(0f, maxOf(0f, dur - window))
                }, valueRange = 0.5f..dur)
            }
            Column(Modifier.weight(1f)) {
                Text("Début : ${"%.2f".fmt(start.toDouble())} s", color = Muted, fontSize = 12.sp)
                Slider(value = start, onValueChange = { start = it }, valueRange = 0f..maxOf(0.001f, dur - window), enabled = window < dur)
            }
        }
        val t0 = start
        val t1 = start + window
        TimePanel("Phonocardiogramme (${r.fs} Hz)", t0, t1, cursor, height = 120) { w, h ->
            drawMinMax(r.samples, r.fs.toDouble(), t0, t1, w, h, Accent, -1f, 1f)
        }
        TimePanel("Spectrogramme 0–500 Hz (dB)", t0, t1, cursor, height = 110) { w, h -> drawSpectrogram(r.spectrogram, t0, t1, w, h) }
        TimePanel("ECG, dérivation II (mêmes battements que le son)", t0, t1, cursor, height = 110) { w, h ->
            drawMinMax(r.ecg, r.ecgFs, t0, t1, w, h, EcgGreen, null, null)
        }
        TimePanel("Pressions (mmHg) : aorte, VG, OG", t0, t1, cursor, height = 150, legend = listOf("Aorte" to AoRed, "VG" to LvBlue, "OG" to LaAmber)) { w, h ->
            val hi = maxOf(r.ao.maxOrNull() ?: 1f, r.lv.maxOrNull() ?: 1f) * 1.05f
            drawMinMax(r.ao, r.pressureFs, t0, t1, w, h, AoRed, 0f, hi)
            drawMinMax(r.lv, r.pressureFs, t0, t1, w, h, LvBlue, 0f, hi)
            drawMinMax(r.la, r.pressureFs, t0, t1, w, h, LaAmber, 0f, hi)
        }
        Text(
            "Son synthétisé (module auscultation, calé sur deux enregistrements) ; pressions : battement simulé posé à " +
                "chaque battement, forme non recalculée battement par battement. Le préréglage 72/min est un réglage d'usage. " +
                "Usage pédagogique uniquement.",
            color = Muted, fontSize = 11.sp,
        )
    }
}

@Composable
private fun Chip(label: String, on: Boolean, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick,
        colors = ButtonDefaults.outlinedButtonColors(
            containerColor = if (on) Accent else Color.Transparent,
            contentColor = if (on) Color.White else Accent,
        ),
    ) { Text(label, fontSize = 13.sp) }
}

/** Panneau à axe de temps commun [t0, t1] avec graduations et curseur de lecture. */
@Composable
private fun TimePanel(
    title: String,
    t0: Float,
    t1: Float,
    cursor: Float?,
    height: Int,
    legend: List<Pair<String, Color>> = emptyList(),
    draw: DrawScope.(Float, Float) -> Unit,
) {
    val measurer = rememberTextMeasurer()
    Column(Modifier.fillMaxWidth().background(Color.White, RoundedCornerShape(12.dp)).padding(10.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(title, fontWeight = FontWeight.SemiBold, color = Ink, fontSize = 13.sp)
            legend.forEach { (l, c) ->
                Box(Modifier.width(16.dp).height(3.dp).background(c))
                Text(l, color = Muted, fontSize = 12.sp)
            }
        }
        Canvas(Modifier.fillMaxWidth().height(height.dp)) {
            val axis = 16f
            val w = size.width
            val h = size.height - axis
            draw(w, h)
            // graduations temporelles
            val span = t1 - t0
            val step = when { span <= 1f -> 0.1f; span <= 3f -> 0.25f; span <= 6f -> 0.5f; else -> 1f }
            var t = (floor(t0 / step) + 1) * step
            while (t < t1) {
                val x = (t - t0) / span * w
                drawLine(Color(0xFFE6EAEE), Offset(x, 0f), Offset(x, h))
                drawText(measurer, "${"%.2f".fmt(t.toDouble()).trimEnd('0').trimEnd(',')} s", Offset(x + 2f, h + 1f), TextStyle(fontSize = 9.sp, color = Muted))
                t += step
            }
            cursor?.takeIf { it in t0..t1 }?.let {
                val x = (it - t0) / span * w
                drawLine(CursorColor, Offset(x, 0f), Offset(x, h), strokeWidth = 2.5f)
            }
        }
    }
}

/** Trace min/max par colonne de pixels (lisible à tout zoom). */
private fun DrawScope.drawMinMax(v: FloatArray, fs: Double, t0: Float, t1: Float, w: Float, h: Float, color: Color, lo0: Float?, hi0: Float?) {
    if (v.isEmpty()) return
    val i0 = (t0 * fs).toInt().coerceIn(0, v.size - 1)
    val i1 = (t1 * fs).toInt().coerceIn(i0 + 1, v.size)
    var lo = lo0 ?: Float.MAX_VALUE
    var hi = hi0 ?: -Float.MAX_VALUE
    if (lo0 == null || hi0 == null) for (i in i0 until i1) { if (lo0 == null) lo = minOf(lo, v[i]); if (hi0 == null) hi = maxOf(hi, v[i]) }
    val span = (hi - lo).takeIf { it > 1e-6f } ?: 1f
    fun y(x: Float) = 4f + (1f - (x - lo) / span) * (h - 8f)
    val cols = w.toInt().coerceAtLeast(1)
    val n = i1 - i0
    if (n <= cols * 2) {
        val path = Path()
        for (i in i0 until i1) {
            val px = (i - i0).toFloat() / (n - 1).coerceAtLeast(1) * w
            if (i == i0) path.moveTo(px, y(v[i])) else path.lineTo(px, y(v[i]))
        }
        drawPath(path, color, style = Stroke(width = 1.5f))
    } else {
        for (c in 0 until cols) {
            val a = i0 + (c.toLong() * n / cols).toInt()
            val b = (i0 + ((c + 1).toLong() * n / cols).toInt()).coerceAtLeast(a + 1)
            var mn = Float.MAX_VALUE
            var mx = -Float.MAX_VALUE
            for (i in a until b) { mn = minOf(mn, v[i]); mx = maxOf(mx, v[i]) }
            drawLine(color, Offset(c.toFloat(), y(mx)), Offset(c.toFloat(), y(mn) + 0.5f), strokeWidth = 1.2f)
        }
    }
}

private fun DrawScope.drawSpectrogram(s: Spectrogram, t0: Float, t1: Float, w: Float, h: Float) {
    if (s.frames == 0) return
    val f0 = (t0 / s.hopS).toInt().coerceIn(0, s.frames - 1)
    val f1 = (t1 / s.hopS).toInt().coerceIn(f0 + 1, s.frames)
    val cw = w / (f1 - f0)
    val ch = h / s.bins
    for (f in f0 until f1) {
        for (b in 0 until s.bins) {
            val level = ((s[f, b] - s.minDb) / (s.maxDb - s.minDb)).coerceIn(0f, 1f)
            drawRect(heat(level), Offset((f - f0) * cw, h - (b + 1) * ch), Size(cw + 0.6f, ch + 0.6f))
        }
    }
}

/** Échelle de couleur sombre → clair (lisible en impression). */
private fun heat(x: Float): Color {
    val r = (0.05f + 0.95f * x).coerceIn(0f, 1f)
    val g = (x * x * 0.9f).coerceIn(0f, 1f)
    val b = (0.25f + 0.5f * (1f - x)).coerceIn(0f, 1f) * (1f - x * 0.6f)
    return Color(r, g, b)
}
