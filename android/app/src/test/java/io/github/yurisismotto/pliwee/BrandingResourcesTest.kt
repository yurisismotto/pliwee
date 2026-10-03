package io.github.yurisismotto.pliwee

import java.io.File
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Android launcher icon wears the Pliwee Flow Monogram, and provably the
 * same one the frozen master draws.
 *
 * Since Wave 6 of the Pliwee rebrand (ADR-0020 §D8) every layer is derived
 * from `docs/design/assets/pliwee-mark.svg` and `pliwee-mark-mono.svg` by
 * `docs/reports/branding/pliwee-wave-6/derive_android_icons.py`, and this test
 * re-reads those masters. The history below is why the test exists; it
 * started out guarding the AnyFlow and OmniBridge marks.
 *
 * ## Why this test exists at all
 *
 * The branding sprint that introduced `logo-flow-a.svg` moved the product's
 * identity mark and rewrote `app-icon.svg` to carry it. The desktop followed;
 * Android did not, and went on shipping the older Flowing Ribbon on its
 * launcher icon. Nothing failed, because nothing was checking — an app icon
 * is the one asset whose correctness is normally established by somebody
 * looking at a home screen.
 *
 * So these are the checks that would have caught it, and they are deliberately
 * the same shape as `desktop/gui/tests/brand_assets.rs`: the icon is compared
 * to the canonical SVG **by its geometry**, not by its filename.
 *
 * ## Why the geometry is recomputed rather than described
 *
 * The safe-zone claims in `ic_launcher_foreground.xml`'s comment are numbers
 * somebody worked out once. A comment cannot fail. So this test parses the
 * drawable's own path and group transform, flattens the curves, and measures
 * where the ink actually lands — which means editing the icon and getting the
 * placement wrong breaks the build rather than the home screen.
 *
 * All of it runs on the JVM: a VectorDrawable is an XML file and a cubic
 * Bézier is arithmetic. Nothing here needs a device.
 */
class BrandingResourcesTest {

    // -----------------------------------------------------------------------
    // Reading the resources. Gradle runs unit tests with the module directory
    // as the working directory.
    // -----------------------------------------------------------------------

    private fun res(path: String): String {
        val file = File("src/main/res/$path")
        assertTrue("expected a resource at ${file.absolutePath}", file.isFile)
        return file.readText()
    }

    private val manifest: String by lazy {
        File("src/main/AndroidManifest.xml").readText()
    }

    /**
     * The manifest and the drawables with their comments removed.
     *
     * Documentation is not a declaration. These files explain at length what
     * they are *not* — the ribbon they no longer use, the gradient the
     * monochrome layer must not have — and a check that could not tell prose
     * from XML would push the next person into deleting the explanation.
     */
    private fun withoutComments(xml: String): String =
        xml.replace(Regex("<!--.*?-->", RegexOption.DOT_MATCHES_ALL), "")

    /** The canonical mark, from `docs/design/` on the test classpath. */
    private fun canonicalSvg(name: String): String =
        javaClass.classLoader!!.getResourceAsStream("assets/$name")
            ?.bufferedReader()?.readText()
            ?: error("docs/design/assets/$name is not on the test classpath")

    /** One of the four paths the master defines once and paints by reference. */
    private fun masterPath(svg: String, id: String): String =
        Regex("""<path id="$id" d="([^"]+)"""").find(canonicalSvg(svg))?.groupValues?.get(1)
            ?: error("$svg defines no path #$id")

    /**
     * The approved mark's outline, read out of the approved file.
     *
     * This one string is what every Android layer has to agree with, and it
     * is taken from the artwork rather than written down here — a constant
     * copied into this file would only prove that the constant matches itself.
     */
    private fun canonicalOutline(): String = masterPath(MASTER, "silhouette")

    /** The three faces the coloured master paints, clipped to the outline. */
    private fun canonicalFaces(): List<String> = FACES.map { masterPath(MASTER, it) }

    /** Every colour the approved mark paints with. */
    private fun canonicalHexes(): List<String> =
        Regex("stop-color=\"#([0-9A-Fa-f]{6})\"")
            .findAll(canonicalSvg(MASTER))
            .map { it.groupValues[1].uppercase() }
            .distinct()
            .toList()

    /**
     * Every gradient stop of the master, as the `<item>` a VectorDrawable
     * must carry for it: the same offset string, and the colour with
     * `stop-opacity` as the alpha byte (round(opacity × 255)).
     */
    private fun canonicalStopItems(): List<String> =
        Regex("""<stop offset="([^"]+)" stop-color="#([0-9A-Fa-f]{6})"(?: stop-opacity="([^"]+)")?/>""")
            .findAll(canonicalSvg(MASTER))
            .map { m ->
                val alpha = Math.round((m.groupValues[3].ifEmpty { "1" }).toDouble() * 255).toInt()
                """<item android:offset="${m.groupValues[1]}" android:color="#%02X%s" />"""
                    .format(alpha, m.groupValues[2].uppercase())
            }
            .toList()

    /**
     * Every radial paint's `gradientTransform`, as the VectorDrawable
     * `<group>` that must reproduce it: translate, rotate and scale are the
     * master's own numbers, byte for byte.
     */
    private fun canonicalRadialGroups(): List<String> =
        Regex("""gradientTransform="translate\(([-\d.]+) ([-\d.]+)\) rotate\(([-\d.]+)\) scale\(([-\d.]+) ([-\d.]+)\)"""")
            .findAll(canonicalSvg(MASTER))
            .map { m ->
                val (tx, ty, rot, sx, sy) = m.destructured
                """<group android:translateX="$tx" android:translateY="$ty" """ +
                    """android:rotation="$rot" android:scaleX="$sx" android:scaleY="$sy">"""
            }
            .toList()

    /** Whitespace-insensitive view of a drawable's markup, for element matching. */
    private fun squeezed(xml: String): String = withoutComments(xml).replace(Regex("\\s+"), " ")

    /**
     * Pulls an attribute off the first element that carries it.
     *
     * The leading boundary is load-bearing. Without it, asking for `d` finds
     * the `d` at the end of `id="pliwee-flow"` and reports a gradient's
     * name as the mark's path data — which is exactly the kind of false
     * green a geometry test must not be capable of.
     */
    private fun attr(xml: String, name: String): String? =
        Regex("""(?<![\w:.-])$name\s*=\s*"([^"]*)"""").find(xml)?.groupValues?.get(1)

    // -----------------------------------------------------------------------
    // Geometry
    // -----------------------------------------------------------------------

    private data class Point(val x: Double, val y: Double)

    /**
     * Flattens a path into the points it covers.
     *
     * `M`, `L`, `C` and `Z` are accepted and nothing else, which is itself
     * part of the assertion: the approved artwork is absolute lines and
     * cubics, so an arc or a relative command would mean something other
     * than the approved geometry is being drawn. The master writes commands
     * directly after numbers (`252.40C218.45`), so tokens are matched rather
     * than split on whitespace. A command letter is followed by exactly one
     * coordinate group; an implicit repeat would be refused as an unexpected
     * token rather than silently mis-read.
     */
    private fun flatten(pathData: String): List<Point> {
        val tokens = Regex("""[A-Za-z]|-?(?:\d+\.?\d*|\.\d+)""").findAll(pathData)
            .map { it.value }.toList()
        val points = mutableListOf<Point>()
        var cursor: Point? = null
        var i = 0
        var moves = 0

        fun number(at: Int): Double = tokens[at].toDoubleOrNull()
            ?: error("unparsable number '${tokens[at]}' in path data")

        while (i < tokens.size) {
            when (tokens[i]) {
                "M" -> {
                    moves++
                    cursor = Point(number(i + 1), number(i + 2))
                    points += cursor
                    i += 3
                }
                "C" -> {
                    val from = cursor ?: error("a curve before any move")
                    val c1 = Point(number(i + 1), number(i + 2))
                    val c2 = Point(number(i + 3), number(i + 4))
                    val to = Point(number(i + 5), number(i + 6))
                    for (step in 1..STEPS) {
                        val t = step.toDouble() / STEPS
                        val m = 1 - t
                        points += Point(
                            m * m * m * from.x + 3 * m * m * t * c1.x +
                                3 * m * t * t * c2.x + t * t * t * to.x,
                            m * m * m * from.y + 3 * m * m * t * c1.y +
                                3 * m * t * t * c2.y + t * t * t * to.y,
                        )
                    }
                    cursor = to
                    i += 7
                }
                "L" -> {
                    cursor = Point(number(i + 1), number(i + 2))
                    points += cursor
                    i += 3
                }
                "Z", "z" -> {
                    // Closes back to the subpath's start, which is already a
                    // point we hold, so there is nothing new to add.
                    i += 1
                }
                else -> error(
                    "unexpected path command '${tokens[i]}': the mark is M, L, C and Z only"
                )
            }
        }
        // The Pliwee silhouette is an outer contour and a hole (Wave 0 report,
        // note for W6). Counted against the master, not written down here.
        val masterMoves = Regex("M").findAll(canonicalOutline()).count()
        assertEquals("the outline has the master's subpaths (outer + hole)", 2, masterMoves)
        assertEquals("the drawn outline has the master's $masterMoves subpaths", masterMoves, moves)
        return points
    }

    /** A `<group>`'s transform, as a VectorDrawable applies it: scale, then translate. */
    private data class Group(val scale: Double, val translateX: Double, val translateY: Double) {
        fun apply(p: Point) = Point(p.x * scale + translateX, p.y * scale + translateY)
    }

    private fun group(xml: String): Group {
        // Comments first. These files explain their own derivation, and that
        // prose legitimately contains the words `<group>` and `android:pathData`
        // — a scanner that reads the explanation instead of the markup finds a
        // group with no attributes and silently measures an untransformed mark.
        val block = Regex("<group[^>]*>", RegexOption.DOT_MATCHES_ALL)
            .find(withoutComments(xml))?.value
            ?: error("the launcher layer draws its mark inside a <group>")
        // A non-zero pivot would change the arithmetic below, and the icon is
        // deliberately written without one.
        assertEquals("pivotX must be left at its default", null, attr(block, "android:pivotX"))
        assertEquals("pivotY must be left at its default", null, attr(block, "android:pivotY"))
        val scaleX = attr(block, "android:scaleX")?.toDouble() ?: 1.0
        val scaleY = attr(block, "android:scaleY")?.toDouble() ?: 1.0
        assertEquals("the mark is never stretched: scaleX and scaleY agree", scaleX, scaleY, 0.0)
        return Group(
            scale = scaleX,
            translateX = attr(block, "android:translateX")?.toDouble() ?: 0.0,
            translateY = attr(block, "android:translateY")?.toDouble() ?: 0.0,
        )
    }

    // =======================================================================
    // B1 — the adaptive icon's layers exist
    // =======================================================================

    @Test
    fun `every adaptive icon layer resolves to a resource that exists`() {
        for (icon in ADAPTIVE_ICONS) {
            val xml = res("mipmap-anydpi-v26/$icon")
            val colours = res("values/colors.xml")

            for (layer in listOf("background", "foreground", "monochrome")) {
                val reference = Regex("""<$layer[^>]*android:drawable="([^"]*)"""")
                    .find(xml)?.groupValues?.get(1)
                    ?: error("$icon has no <$layer> layer")

                when {
                    reference.startsWith("@drawable/") -> assertTrue(
                        "$icon's $layer names @drawable/${reference.removePrefix("@drawable/")}, " +
                            "which is not in res/drawable",
                        File("src/main/res/drawable/${reference.removePrefix("@drawable/")}.xml")
                            .isFile,
                    )
                    reference.startsWith("@color/") -> assertTrue(
                        "$icon's $layer names $reference, which colors.xml does not define",
                        colours.contains("name=\"${reference.removePrefix("@color/")}\""),
                    )
                    else -> error("$icon's $layer is neither a drawable nor a colour: $reference")
                }
            }
        }
    }

    // =======================================================================
    // B2 — the themed icon is declared
    // =======================================================================

    @Test
    fun `both launcher icons declare a monochrome layer`() {
        for (icon in ADAPTIVE_ICONS) {
            assertTrue(
                "$icon has no <monochrome> layer, so Android 13+ themed icons " +
                    "fall back to a generic tile",
                withoutComments(res("mipmap-anydpi-v26/$icon")).contains("<monochrome"),
            )
        }
    }

    // =======================================================================
    // B3 — the manifest points at them
    // =======================================================================

    @Test
    fun `the manifest names launcher icons that exist`() {
        val declared = withoutComments(manifest)
        assertEquals("@mipmap/ic_launcher", attr(declared, "android:icon"))
        assertEquals("@mipmap/ic_launcher_round", attr(declared, "android:roundIcon"))
        for (icon in ADAPTIVE_ICONS) {
            assertTrue(
                "the manifest names @mipmap/${icon.removeSuffix(".xml")} and it is missing",
                File("src/main/res/mipmap-anydpi-v26/$icon").isFile,
            )
        }
    }

    // =======================================================================
    // B4 — nothing still points at the mark this replaced
    // =======================================================================

    @Test
    fun `no launcher resource still draws a retired mark`() {
        for (layer in LAUNCHER_LAYERS + ADAPTIVE_ICONS.map { "mipmap-anydpi-v26/$it" }) {
            val drawn = withoutComments(res(layer))
            for (dead in listOf("ribbon", "flow_a", "flow-a", "anyflow", "flowing", "omnibridge")) {
                assertFalse(
                    "$layer still references the retired mark '$dead'",
                    drawn.contains(dead, ignoreCase = true),
                )
            }
        }

        // The retired marks are gone rather than merely unused: two brand
        // marks in one res/ directory is how a screen quietly keeps wearing
        // the wrong one.
        for (dead in listOf(
            "logo_flow_a.xml", "logo_flowing_a.xml", "logo_flowing_ribbon.xml",
            "logo_omnibridge_mark.xml",
        )) {
            assertFalse(
                "src/main/res/drawable/$dead is still present; the Pliwee mark replaced it",
                File("src/main/res/drawable/$dead").exists(),
            )
        }
        val sources = File("src/main/java").walkTopDown()
            .filter { it.isFile && it.extension == "kt" }
        for (source in sources) {
            for (dead in listOf(
                "logo_flowing_a", "logo_flow_a", "logo_flowing_ribbon", "logo_omnibridge_mark",
            )) {
                assertFalse(
                    "${source.name} still asks for R.drawable.$dead",
                    source.readText().contains(dead),
                )
            }
        }
    }

    // =======================================================================
    // B5 — the mark is the canonical one, and it is not clipped
    // =======================================================================

    @Test
    fun `every launcher layer carries the canonical Pliwee outline`() {
        val canonical = canonicalOutline()
        assertTrue(
            "the canonical outline is implausibly short, so this test proves nothing",
            canonical.length > 2000,
        )
        // The mono master paints this same path and nothing else, so the
        // themed layer derived from it is held to the same string.
        assertEquals(
            "pliwee-mark-mono.svg's silhouette differs from pliwee-mark.svg's",
            canonical,
            masterPath(MONO_MASTER, "silhouette"),
        )

        // Geometry, not filenames. A drawable called ic_launcher_foreground
        // that drew something else would pass a name check and fail this.
        for (layer in LAUNCHER_LAYERS + BRAND_MARK) {
            val drawn = withoutComments(res(layer))
            assertTrue(
                "$layer has been redrawn rather than derived from pliwee-mark.svg",
                drawn.contains(canonical),
            )
        }

        // The coloured layers must also carry the three faces, or they are
        // the silhouette alone wearing the mark's name.
        val faces = canonicalFaces()
        assertEquals(3, faces.size)
        for (face in faces) {
            assertTrue("a master face is implausibly short", face.length > 2000)
            for (layer in COLOURED_LAYERS) {
                assertTrue(
                    "$layer is missing one of the canonical faces",
                    withoutComments(res(layer)).contains(face),
                )
            }
        }

        // And the themed layer draws the silhouette alone: exactly one path.
        val monoPaths = Regex("android:pathData=\"([^\"]*)\"")
            .findAll(withoutComments(res("drawable/ic_launcher_monochrome.xml")))
            .map { it.groupValues[1] }
            .toList()
        assertEquals("the monochrome layer draws one path, the silhouette", listOf(canonical), monoPaths)
    }

    /**
     * The paint is the master's paint: every stop of every gradient, and
     * every radial's transform, appears in the coloured layers exactly as the
     * master writes it. Nothing is approximated, so nothing is re-coloured.
     */
    @Test
    fun `the coloured layers carry every master gradient stop and radial transform`() {
        val stops = canonicalStopItems()
        val radials = canonicalRadialGroups()
        assertTrue("the master has implausibly few stops: ${stops.size}", stops.size >= 30)
        assertEquals("the master's radial paints", 9, radials.size)
        for (layer in COLOURED_LAYERS) {
            val drawn = squeezed(res(layer))
            for (item in stops) {
                assertTrue("$layer lacks the master stop $item", drawn.contains(item))
            }
            for (group in radials) {
                assertTrue("$layer lacks the master radial transform $group", drawn.contains(group))
            }
            // One gradient per master paint: 4 linear + 9 radial.
            assertEquals(
                "$layer declares a different number of gradients than the master paints",
                13,
                Regex("<gradient ").findAll(drawn).count(),
            )
        }
    }

    @Test
    fun `the mark stays inside every launcher mask`() {
        for (layer in LAUNCHER_LAYERS) {
            val xml = withoutComments(res(layer))
            assertEquals("the adaptive canvas is 108 units", "108", attr(xml, "android:viewportWidth"))
            assertEquals("the adaptive canvas is 108 units", "108", attr(xml, "android:viewportHeight"))

            val transform = group(xml)
            // The mark is filled, not stroked, so its ink is exactly its
            // outline — there is no half-stroke of overshoot to allow for.
            assertEquals(
                "$layer paints a stroke; the approved mark is filled",
                null,
                attr(xml, "android:strokeWidth"),
            )
            // The outline encloses every overlay, because the overlays are
            // clipped to it, so measuring the outline measures the ink.
            val ink = flatten(attr(xml, "android:pathData")!!).map(transform::apply)

            val left = ink.minOf { it.x }
            val right = ink.maxOf { it.x }
            val top = ink.minOf { it.y }
            val bottom = ink.maxOf { it.y }
            val squareMargin = minOf(left - SAFE_MIN, top - SAFE_MIN, SAFE_MAX - right, SAFE_MAX - bottom)
            assertTrue(
                "$layer leaves the 72x72 safe zone: ink spans x[$left, $right] y[$top, $bottom]",
                squareMargin >= 0,
            )

            // The round mask is stricter than the square one and is the one
            // that actually clips a mark whose bounding box fits: Android
            // guarantees only a 66-unit circle about the centre.
            val radius = ink.maxOf { hypot(it.x - CENTRE, it.y - CENTRE) }
            assertTrue(
                "$layer leaves the 66-unit round safe zone: ink reaches $radius of $ROUND_LIMIT",
                radius <= ROUND_LIMIT,
            )

            // And it should not be so small that the tile reads as mostly
            // background. This is the other half of "not clipped".
            val span = max(right - left, bottom - top)
            assertTrue(
                "$layer's mark spans only $span of the 72-unit safe zone",
                span >= 0.7 * (SAFE_MAX - SAFE_MIN),
            )
        }
    }

    @Test
    fun `the icon carries no text`() {
        for (layer in LAUNCHER_LAYERS) {
            // A VectorDrawable cannot render text at all — there is no
            // element for it — so the only way text reaches a launcher icon
            // is as a raster. Both are refused below and in `B7`.
            assertFalse(layer, withoutComments(res(layer)).contains("<text"))
        }
    }

    // =======================================================================
    // B6 — the themed layer is genuinely monochrome
    // =======================================================================

    @Test
    fun `the monochrome layer names no colour of its own`() {
        val drawn = withoutComments(res("drawable/ic_launcher_monochrome.xml"))

        assertFalse(
            "a themed icon is tinted by the system; a gradient in it is thrown away",
            drawn.contains("gradient", ignoreCase = true),
        )
        for (brand in canonicalHexes()) {
            assertFalse(
                "the monochrome layer paints with brand colour $brand",
                drawn.contains(brand, ignoreCase = true),
            )
        }
        // Whatever colours it does name must all be the same one.
        val colours = Regex("#[0-9A-Fa-f]{6,8}").findAll(drawn)
            .map { it.value.uppercase() }
            .toSet()
        assertEquals("the monochrome layer uses exactly one colour: $colours", 1, colours.size)
    }

    @Test
    fun `the coloured foreground paints with the approved gradients and nothing else`() {
        val drawn = withoutComments(res("drawable/ic_launcher_foreground.xml"))
        // Every colour the foreground names, without its alpha, must be a
        // colour the approved artwork names. Derived from the SVG rather than
        // listed here, so a stop that drifts fails instead of being re-pinned.
        val approved = canonicalHexes().toSet()
        val used = Regex("#[0-9A-Fa-f]{8}").findAll(drawn)
            .map { it.value.substring(3).uppercase() }
            .toSet()
        assertEquals(
            "the foreground paints with colours the approved mark does not define",
            emptySet<String>(),
            used - approved,
        )
        assertEquals(
            "the foreground leaves out colours the approved mark paints with",
            emptySet<String>(),
            approved - used,
        )
        // Surface, and from the one place the platform can read before Compose
        // runs. Dark until 2026-10-03, when the owner found the black tile
        // heavy; Surface is also the window background, so the splash and the
        // icon are one colour.
        assertTrue(
            "the launcher background is the brand Surface",
            res("values/colors.xml").contains("""<color name="ic_launcher_background">#F7F9FC</color>"""),
        )
    }

    // =======================================================================
    // B7 — nothing is fetched, and nothing is a screenshot
    // =======================================================================

    @Test
    fun `the launcher icon is drawn, not downloaded and not photographed`() {
        val icons = File("src/main/res").walkTopDown()
            .filter { it.isFile && it.name.startsWith("ic_launcher") }
            .toList()
        assertTrue("no launcher resources were found at all", icons.isNotEmpty())

        for (icon in icons) {
            assertEquals(
                "${icon.name} is a bitmap; a launcher icon is vector artwork",
                "xml",
                icon.extension,
            )
            val text = icon.readText()
            assertFalse("${icon.name} embeds a raster", text.contains("<bitmap"))
            // Namespace declarations are URLs but are not references to
            // anything: `xmlns:android="http://schemas.android.com/..."` names
            // a vocabulary, and nothing is ever fetched from it.
            val fetchable = withoutComments(text)
                .replace(Regex("""xmlns:[\w-]+\s*=\s*"[^"]*""""), "")
            assertFalse(
                "${icon.name} reaches outside the APK",
                Regex("https?://|content://|file://").containsMatchIn(fetchable),
            )
        }

        // And no image-loading dependency was added to draw them with.
        val build = File("build.gradle.kts").readText()
        for (library in listOf("coil", "glide", "picasso", "fresco")) {
            assertFalse(
                "$library was added; a vector drawable needs no runtime image library",
                build.contains(library, ignoreCase = true),
            )
        }
    }

    // =======================================================================
    // B8 — one manifest, so debug and release cannot disagree
    // =======================================================================

    @Test
    fun `no build variant overrides the application icon`() {
        for (variant in listOf("debug", "release")) {
            assertFalse(
                "src/$variant/AndroidManifest.xml exists and could rename or re-icon the app",
                File("src/$variant/AndroidManifest.xml").exists(),
            )
            assertFalse(
                "src/$variant/res exists and could shadow the launcher icon",
                File("src/$variant/res").exists(),
            )
        }
        // A manifest placeholder would do the same thing from inside Gradle.
        val build = File("build.gradle.kts").readText()
        assertFalse(
            "a manifest placeholder can vary android:icon per build type",
            build.contains("manifestPlaceholders"),
        )
    }

    @Test
    fun `the app label is the product name and comes from resources`() {
        assertEquals("@string/app_name", attr(withoutComments(manifest), "android:label"))
        assertTrue(
            "the launcher entry must read Pliwee",
            res("values/strings.xml").contains("""<string name="app_name">Pliwee</string>"""),
        )
    }

    // =======================================================================
    // B9 — what Compose draws is what the platform draws
    // =======================================================================

    /**
     * Every drawable the app paints through Compose clips each path exactly
     * as the platform VectorDrawable renderer clips it.
     *
     * The two renderers read `<clip-path>` differently. The platform keeps the
     * clips per `<group>` and intersects nested ones. Compose's XML parser
     * (androidx.compose.ui 1.7.6, `parseCurrentVectorNode`) keeps one counter
     * for the whole document and, at the first `</group>` of any kind, closes
     * every clip opened so far. Pliwee 1.1.0's `logo_pliwee_mark.xml` nested
     * a face clip inside the silhouette clip and then a radial's transform
     * group inside that: Compose dropped both clips after the first radial and
     * painted the rest unclipped across the app bar and Settings, while the
     * launcher icon — the platform renderer, from the same conversion — was
     * right. A launcher check could not see it, and nothing else looked.
     *
     * So both readings are modelled here and compared path by path, for every
     * drawable the Kotlin sources name. `BrandMarkRenderParityTest` measures
     * the same thing in pixels, on a device.
     */
    @Test
    fun `every drawable Compose paints is clipped as the platform clips it`() {
        val sources = File("src/main/java").walkTopDown().filter { it.extension == "kt" }.toList()
        assertTrue("found no Kotlin sources, so nothing names a drawable", sources.size > 10)
        val names = sources
            .flatMap { f -> Regex("""(?<![\w.])R\.drawable\.(\w+)""").findAll(f.readText()).map { it.groupValues[1] }.toList() }
            .toSortedSet()
        assertTrue("the brand mark must be among the drawables the code paints", "logo_pliwee_mark" in names)

        var clippedPaths = 0
        for (name in names) {
            val xml = res("drawable/$name.xml")
            val platform = clipsPerPath(xml, compose = false)
            val compose = clipsPerPath(xml, compose = true)
            assertEquals("$name: the two readings saw a different number of paths", platform.size, compose.size)
            for (i in platform.indices) {
                assertEquals(
                    "$name: path #$i is clipped differently by Compose than by the platform renderer",
                    platform[i],
                    compose[i],
                )
            }
            clippedPaths += platform.count { it.isNotEmpty() }
        }
        // The mark alone has 12 clipped layers. Fewer means the model saw
        // nothing to compare, and a pass over nothing is not a pass.
        assertTrue("only $clippedPaths clipped paths were compared", clippedPaths >= 12)
    }

    /**
     * For each `<path>` in document order, the set of `<clip-path>` data that
     * clips it — under the platform's per-group rule, or under Compose's
     * single document-wide counter.
     */
    private fun clipsPerPath(xml: String, compose: Boolean): List<Set<String>> {
        val stack = ArrayDeque<MutableList<String>>().apply { addLast(mutableListOf()) }
        var openClips = 0
        val out = mutableListOf<Set<String>>()
        fun endGroup() {
            if (compose) {
                // Never below the root: an over-pop is a mismatch, reported below.
                repeat(openClips + 1) { if (stack.size > 1) stack.removeLast() }
                openClips = 0
            } else {
                stack.removeLast()
            }
        }
        // Attribute values in a VectorDrawable never contain '>', so a tag is
        // everything between '<' and the next '>'.
        for (tag in Regex("<(/?)([\\w-]+)([^>]*?)(/?)>").findAll(withoutComments(xml))) {
            val (close, name, attrs, selfClosing) = tag.destructured
            val data = Regex("android:pathData=\"([^\"]*)\"").find(attrs)?.groupValues?.get(1)
            when {
                close.isNotEmpty() -> if (name == "group") endGroup()
                name == "group" -> {
                    stack.addLast(if (compose) mutableListOf() else stack.last().toMutableList())
                    if (selfClosing.isNotEmpty()) endGroup()
                }
                name == "clip-path" -> if (compose) {
                    // Compose pushes a clipping group of its own.
                    stack.addLast(mutableListOf(data!!)); openClips++
                } else {
                    stack.last().add(data!!)
                }
                name == "path" -> out += if (compose) stack.flatten().toSet() else stack.last().toSet()
            }
        }
        if (!compose) assertEquals("unbalanced <group>s", 1, stack.size)
        return out
    }

    private companion object {
        /** Points per curve when flattening. Far finer than the grid. */
        const val STEPS = 400

        /** The 108-unit canvas, and the inner 72 every mask shows. */
        const val SAFE_MIN = 18.0
        const val SAFE_MAX = 90.0
        const val CENTRE = 54.0

        /**
         * Android guarantees only a 66-unit circle about the centre, which is
         * stricter than the square and is what clips a mark whose bounding
         * box fits but whose corners do not.
         */
        const val ROUND_LIMIT = 33.0

        val ADAPTIVE_ICONS = listOf("ic_launcher.xml", "ic_launcher_round.xml")

        val LAUNCHER_LAYERS = listOf(
            "drawable/ic_launcher_foreground.xml",
            "drawable/ic_launcher_monochrome.xml",
        )

        /** The in-app brand drawable, at the master's own viewport. */
        val BRAND_MARK = listOf("drawable/logo_pliwee_mark.xml")

        val COLOURED_LAYERS = listOf("drawable/ic_launcher_foreground.xml") + BRAND_MARK

        /** The frozen masters (Wave 0), on the test classpath via docs/design. */
        const val MASTER = "pliwee-mark.svg"
        const val MONO_MASTER = "pliwee-mark-mono.svg"

        val FACES = listOf("face-loop", "face-tail", "face-sweep")

    }
}
