package fr.vetsim.demo

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.pow

// Éléments visuels communs aux onglets (mêmes valeurs que web/styles.css).

internal val UiAccent = Color(0xFF0B5E7A)
internal val UiBg = Color(0xFFF3F5F7)
internal val UiLine = Color(0xFFDDE3E8)
internal val UiGrid = Color(0xFFE6EAEE)
internal val UiMuted = Color(0xFF5B6B77)
internal val UiInk = Color(0xFF1B2A35)

/** Thème de l'application (aussi utilisé par les captures d'écran des vues isolées). */
@Composable
fun VetTheme(content: @Composable () -> Unit) {
    // conteneurs secondaires (pistes des curseurs, etc.) dans la teinte de l'accent, pas le violet par défaut
    val soft = Color(0xFFD6E6ED)
    MaterialTheme(
        colorScheme = lightColorScheme(
            primary = UiAccent, secondary = UiAccent, secondaryContainer = soft, primaryContainer = soft,
            surfaceVariant = soft, onSurfaceVariant = UiMuted, outline = Color(0xFF9FB0BC),
        ),
        content = content,
    )
}

/** Carte : fond blanc, bord fin, coins arrondis. */
internal fun Modifier.card(): Modifier =
    this.background(Color.White, RoundedCornerShape(12.dp)).border(1.dp, UiLine, RoundedCornerShape(12.dp))

/** Tuile d'une constante clé (valeur du modèle, sans intervalle de référence). */
@Composable
internal fun VitalTile(label: String, value: String, unit: String, modifier: Modifier = Modifier) {
    Column(modifier.card().padding(horizontal = 12.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(label, color = UiMuted, fontSize = 12.sp)
        Text(
            androidx.compose.ui.text.buildAnnotatedString {
                append(value)
                pushStyle(androidx.compose.ui.text.SpanStyle(fontSize = 12.sp, fontWeight = FontWeight.Normal, color = UiMuted))
                append(" $unit")
                pop()
            },
            color = UiInk, fontSize = 22.sp, fontWeight = FontWeight.SemiBold,
        )
    }
}

/** Graduations de pas 1, 2 ou 5 × 10ⁿ comprises dans [lo, hi], et leur format (virgule décimale). */
internal class NiceTicks(val values: List<Float>, private val decimals: Int) {
    fun label(v: Float): String = "%.${decimals}f".fmt(v.toDouble()).replace('-', '−')
}

internal fun niceTicks(lo: Float, hi: Float, n: Int): NiceTicks {
    val span = hi - lo
    if (!(span > 1e-12f) || !span.isFinite()) return NiceTicks(listOf(lo), 2)
    val raw = span / n
    val p = 10.0.pow(floor(log10(raw.toDouble()))).toFloat()
    val m = raw / p
    // pas rond le plus proche du pas brut (1, 2, 5 ou 10 × 10ⁿ)
    val step = (if (m < 1.5f) 1f else if (m < 3f) 2f else if (m < 7f) 5f else 10f) * p
    val out = mutableListOf<Float>()
    var v = ceil(lo / step - 1e-6f) * step
    while (v <= hi + step * 1e-6f) { out += if (abs(v) < step * 1e-6f) 0f else v; v += step }
    val decimals = maxOf(0, -floor(log10(step.toDouble()) + 1e-9).toInt())
    return NiceTicks(out, decimals)
}

/**
 * Axe vertical : graduations rondes dans [lo, hi] (libellés dans la marge gauche, de largeur
 * `left`), lignes de grille si `grid`. `y` convertit une valeur en ordonnée.
 */
internal fun DrawScope.drawYAxis(
    measurer: TextMeasurer, lo: Float, hi: Float, left: Float, right: Float, h: Float,
    y: (Float) -> Float, grid: Boolean = true,
) {
    val t = niceTicks(lo, hi, if (h < 100f) 3 else if (h < 140f) 4 else 5)
    for (v in t.values) {
        val yy = y(v)
        if (grid) drawLine(UiGrid, Offset(left, yy), Offset(right, yy))
        val lab = measurer.measure(t.label(v), TextStyle(fontSize = 9.sp, color = UiMuted))
        val top = (yy - lab.size.height / 2f).coerceIn(0f, maxOf(0f, h - lab.size.height))
        drawText(lab, topLeft = Offset(2f, top))
    }
}
