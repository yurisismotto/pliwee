package io.github.yurisismotto.pliwee

import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlin.math.abs
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The in-app brand mark, drawn by Compose, is the mark the platform draws.
 * Requires a device.
 *
 * ## What broke
 *
 * Pliwee 1.1.0 drew `logo_pliwee_mark` in the app bar and on Settings with a
 * dark streak across its top and a haze in and under the loop. The launcher
 * icon — same conversion, platform renderer — was right. Compose's vector
 * parser closes every open `<clip-path>` at the first `</group>`, so all but
 * the first radial paint of each face was painted unclipped.
 *
 * `BrandingResourcesTest` models both clip rules on the JVM. This test looks
 * at pixels: it paints the drawable once through `painterResource`, in a real
 * window, and once through the platform `VectorDrawable`, and requires the
 * two to agree. Against the 1.1.0 drawable they disagreed on 83,317 of
 * 281,520 pixels (SM-X620, Android 16, 2026-10-03); against the fix, on none.
 */
@RunWith(AndroidJUnit4::class)
class BrandMarkRenderParityTest {

    @get:Rule val rule = createComposeRule()

    @Test
    fun composeDrawsTheBrandMarkAsThePlatformDoes() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        rule.setContent {
            Box(Modifier.testTag(TAG).background(Color.White)) {
                Image(painterResource(R.drawable.logo_pliwee_mark), null, Modifier.size(276.dp, 255.dp))
            }
        }
        val compose = rule.onNodeWithTag(TAG).captureToImage().asAndroidBitmap()
            .copy(Bitmap.Config.ARGB_8888, false)
        val w = compose.width
        val h = compose.height
        assertTrue("the capture is implausibly small: ${w}x$h", w >= 276 && h >= 255)

        val platform = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        platform.eraseColor(android.graphics.Color.WHITE)
        ContextCompat.getDrawable(context, R.drawable.logo_pliwee_mark)!!.apply {
            setBounds(0, 0, w, h)
            draw(Canvas(platform))
        }

        val a = IntArray(w * h).also { compose.getPixels(it, 0, w, 0, 0, w, h) }
        val b = IntArray(w * h).also { platform.getPixels(it, 0, w, 0, 0, w, h) }

        // The measurement precondition: the platform drew the mark. The master
        // inks ~59 % of its viewBox; a blank or failed draw would make every
        // comparison below agree with nothing.
        val ink = b.count { it != WHITE }
        assertTrue("the platform drew only $ink of ${w * h} pixels", ink > w * h * 45 / 100)

        // 16/255 per channel absorbs anti-aliasing and gradient dithering
        // between the two pipelines; the 1.1.0 defect is far outside it.
        val differing = a.indices.count { channelDelta(a[it], b[it]) > 16 }
        assertTrue(
            "Compose and the platform disagree on $differing of ${w * h} pixels",
            differing <= w * h / 1000,
        )
    }

    private fun channelDelta(p: Int, q: Int): Int =
        maxOf(
            abs((p ushr 16 and 0xFF) - (q ushr 16 and 0xFF)),
            abs((p ushr 8 and 0xFF) - (q ushr 8 and 0xFF)),
            abs((p and 0xFF) - (q and 0xFF)),
        )

    private companion object {
        const val TAG = "brand-mark"
        const val WHITE = -0x1 // opaque white, ARGB
    }
}
