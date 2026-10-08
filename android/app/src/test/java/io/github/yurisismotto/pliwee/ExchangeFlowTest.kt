package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.proto.Platform
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.UiMapping
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The exchange flows — Send clipboard, and the Sharesheet's file and text
 * sends — held to the things that make them truthful.
 *
 * ## Why half of this reads a source file
 *
 * The screens themselves are compositions, and an instrumented run costs a
 * reinstall and a manual re-pairing. So every *decision* lives in
 * [UiMapping] and is tested here directly, and the handful of properties
 * that are genuinely about the rendering — that the destination is never a
 * literal, that the security footer never grows a claim, that the hero is
 * hidden from assistive technology — are asserted against the source, which
 * is the same approach `BrandingResourcesTest` takes to the launcher icons.
 *
 * A source assertion is weaker than a rendered one and is used only where
 * the alternative is no assertion at all. Each one below is a property a
 * reviewer would otherwise have to re-check by eye on every change.
 */
class ExchangeFlowTest {

    private val peerName = "workstation"

    private fun peer(
        hex: String = "ab".repeat(32),
        name: String = peerName,
    ) = TrustStore.TrustedPeer(
        deviceId = "d0",
        deviceName = name,
        fingerprint = Fingerprint.fromHex(hex)!!,
        pairedAtUnix = 0,
        grantedCapabilities = setOf(ClipboardCapability.ID),
        addressHints = emptyList(),
        clipboardPolicy = ClipboardPolicy(),
    )

    private fun session(
        peer: TrustStore.TrustedPeer,
        platform: Platform = Platform.PLATFORM_LINUX,
    ) = PliweeApp.LiveSession(
        peerHex = peer.fingerprint.toHex(),
        negotiated = setOf(ClipboardCapability.ID),
        platform = platform,
    )

    private fun source(name: String): String {
        val file = File("src/main/java/io/github/yurisismotto/pliwee/$name")
        assertTrue("expected a source file at ${file.absolutePath}", file.isFile)
        return file.readText()
    }

    /**
     * The code, without the prose about it.
     *
     * These files explain themselves at length, and the explanations quote
     * the very literals this test forbids — "a screen that printed
     * `Desktop · Linux` at every peer", "deliberately not end-to-end
     * encrypted". A scan that could not tell a comment from a string would
     * fail on a file for correctly describing why it does not do the thing.
     */
    private fun code(name: String): String = withoutComments(source(name))

    private fun withoutComments(text: String): String {
        val out = StringBuilder()
        var i = 0
        var inString = false
        while (i < text.length) {
            val c = text[i]
            val next = text.getOrNull(i + 1)
            when {
                inString -> {
                    out.append(c)
                    if (c == '\\') {
                        text.getOrNull(i + 1)?.let { out.append(it) }
                        i++
                    } else if (c == '"') {
                        inString = false
                    }
                    i++
                }
                c == '"' -> { inString = true; out.append(c); i++ }
                c == '/' && next == '/' -> {
                    while (i < text.length && text[i] != '\n') i++
                }
                c == '/' && next == '*' -> {
                    i += 2
                    while (i < text.length && !(text[i] == '*' && text.getOrNull(i + 1) == '/')) i++
                    i += 2
                }
                else -> { out.append(c); i++ }
            }
        }
        return out.toString()
    }

    private val exchangeScreens = listOf(
        "ui/SendClipboardScreen.kt",
        "ui/SendActivity.kt",
        "ui/components/Exchange.kt",
    )

    // ---- the device glyph is a claim, so it comes from state -------------

    /**
     * A stated platform picks the glyph the hero draws.
     *
     * The mapping is here rather than on a drawable so that it can be tested
     * without a device, and so that [UiMapping] stays free of resource ids.
     */
    @Test
    fun `a stated platform decides the device kind`() {
        assertEquals(
            UiMapping.DeviceKind.Desktop,
            UiMapping.deviceKind(Platform.PLATFORM_LINUX),
        )
        assertEquals(
            UiMapping.DeviceKind.Mobile,
            UiMapping.deviceKind(Platform.PLATFORM_ANDROID),
        )
    }

    /**
     * Nothing stated means nothing drawn about the platform.
     *
     * The hero has a destination tile whatever happens, so an unspecified
     * platform must resolve to a kind of its own rather than to the likeliest
     * one. A desktop glyph here would be the "Desktop · Linux at every peer"
     * defect again, one layer down and harder to see.
     */
    @Test
    fun `an unstated platform is Unknown rather than the likeliest one`() {
        assertEquals(
            UiMapping.DeviceKind.Unknown,
            UiMapping.deviceKind(Platform.PLATFORM_UNSPECIFIED),
        )
        assertEquals(UiMapping.DeviceKind.Unknown, UiMapping.deviceKind(null))
    }

    /** With a live session of its own, the peer is drawn as what it said. */
    @Test
    fun `a peer with its own live session is drawn as what it said`() {
        val p = peer()
        assertEquals(
            UiMapping.DeviceKind.Desktop,
            UiMapping.peerDeviceKind(p, session(p)),
        )
        assertEquals(
            UiMapping.DeviceKind.Mobile,
            UiMapping.peerDeviceKind(p, session(p, Platform.PLATFORM_ANDROID)),
        )
    }

    /**
     * Another computer's session says nothing about this one.
     *
     * The same identity comparison the send gate makes. With two paired
     * desktops, a session with A must not decide how B is drawn.
     */
    @Test
    fun `another peers session does not decide this peers glyph`() {
        val mine = peer(hex = "ab".repeat(32))
        val other = peer(hex = "cd".repeat(32), name = "laptop")

        assertEquals(
            UiMapping.DeviceKind.Unknown,
            UiMapping.peerDeviceKind(mine, session(other)),
        )
    }

    /** No session at all is Unknown, not the last thing anybody said. */
    @Test
    fun `no session means no platform claim`() {
        assertEquals(UiMapping.DeviceKind.Unknown, UiMapping.peerDeviceKind(peer(), null))
    }

    /**
     * The glyph and the identity line agree about what is known.
     *
     * They are read from the same session by the same rule, and a screen
     * showing a monitor above the words "149F 6B66…" would be making a claim
     * in pictures that the text below it declines to make.
     */
    @Test
    fun `the glyph and the identity line agree about what is known`() {
        val p = peer()
        val withSession = UiMapping.peerDeviceKind(p, session(p)) to
            UiMapping.peerIdentityLine(p, session(p))
        val without = UiMapping.peerDeviceKind(p, null) to
            UiMapping.peerIdentityLine(p, null)

        assertEquals(UiMapping.DeviceKind.Desktop, withSession.first)
        assertEquals("Desktop · Linux", withSession.second)

        assertEquals(UiMapping.DeviceKind.Unknown, without.first)
        assertEquals(p.fingerprint.toDisplayShort(), without.second)
    }

    // ---- the primary action names a live destination ---------------------

    /**
     * The idle button names the computer, and the name is the peer's own.
     *
     * The reference shows "Send files to fedora"; what makes that honest is
     * that "fedora" is assembled from `TrustedPeer.deviceName` at the call
     * site rather than typed into the screen.
     */
    @Test
    fun `the idle send button carries the destination it was given`() {
        val label = UiMapping.sendButtonLabel(
            UiMapping.SendAttempt.Idle,
            idleLabel = "Send file to ${peer().deviceName}",
        )
        assertEquals("Send file to $peerName", label)
    }

    /**
     * Once an attempt is under way the button reports the attempt.
     *
     * A button still offering "Send file to workstation" while that very send
     * is in flight would be describing something that has already happened —
     * the property issue #12 was about, and the reason the destination name
     * is confined to the idle state.
     */
    @Test
    fun `an attempt in flight stops offering the destination`() {
        val named = "Send file to $peerName"
        val inFlight = listOf(
            UiMapping.SendAttempt.Sending,
            UiMapping.SendAttempt.Sent,
        )
        for (attempt in inFlight) {
            val label = UiMapping.sendButtonLabel(attempt, idleLabel = named)
            assertEquals("Sending…", label)
            assertFalse("$attempt must not still offer the destination", label.contains(peerName))
        }
        assertEquals(
            "Try again",
            UiMapping.sendButtonLabel(UiMapping.SendAttempt.Failed("nope"), idleLabel = named),
        )
    }

    /** The old call sites keep the label they had. */
    @Test
    fun `the default idle label is unchanged`() {
        assertEquals("Send", UiMapping.sendButtonLabel(UiMapping.SendAttempt.Idle))
    }

    // ---- the CTA is off until a send could actually work ------------------

    /**
     * An empty clipboard leaves the primary action disabled and explained.
     *
     * The gate is [SendClipboardUiTest]'s subject in full; this is the part
     * the redesign could have broken — the button and its reason are one
     * value, so a screen cannot render an enabled button beside the sentence
     * saying why it is off.
     */
    @Test
    fun `an empty clipboard leaves the action off with a reason`() {
        val p = peer()
        val ui = UiMapping.sendClipboardUi(
            UiMapping.clipboardSendGate(p, session(p)),
            preview = null,
        )
        assertFalse(ui.enabled)
        assertFalse(ui.hasContent)
        assertTrue("a disabled action must say why", !ui.blockedReason.isNullOrBlank())
    }

    // ---- nothing on these screens is a stand-in --------------------------

    /**
     * No exchange screen contains a device name or a platform literal.
     *
     * The approved reference shows "Connected to fedora" and "Desktop ·
     * Linux". Both are live values — `deviceName` off the trust store and
     * [UiMapping.peerIdentityLine] off the session — and the one way this
     * design could quietly become untrue is a screen typing them in to look
     * like the mockup.
     *
     * `platformLabel` in [UiMapping] is where "Desktop · Linux" is allowed to
     * exist, exactly once, behind a real `Platform` value.
     */
    @Test
    fun `no exchange screen hardcodes a peer name or a platform`() {
        val forbidden = listOf("fedora", "Desktop · Linux", "anyflow-kde", "87%")
        for (name in exchangeScreens) {
            val text = code(name)
            for (literal in forbidden) {
                assertFalse(
                    "$name contains the literal \"$literal\"; every such value " +
                        "must come from live state",
                    text.contains(literal, ignoreCase = true),
                )
            }
        }
    }

    /**
     * The platform sentence exists in exactly one place.
     *
     * If a second appears, two screens can disagree about what a peer runs,
     * which is how the original defect was able to hide.
     */
    @Test
    fun `the platform wording has exactly one home`() {
        val mapping = code("ui/UiMapping.kt")
        assertEquals(
            "\"Desktop · Linux\" must be written once, in platformLabel",
            1,
            Regex("\"Desktop · Linux\"").findAll(mapping).count(),
        )
    }

    // ---- the security footer may not grow a claim -------------------------

    /**
     * The footer says what the implementation does and not one word more.
     *
     * A direct, mutually authenticated TLS 1.3 session pinned to the key
     * approved at pairing is what these four facts describe. "End-to-end
     * encrypted", "zero knowledge" and friends describe something else, and
     * a footer is exactly where such a phrase gets added because it sounds
     * like a synonym.
     */
    @Test
    fun `the security footer states the four approved facts`() {
        val exchange = source("ui/components/Exchange.kt")
        for (fact in listOf(
            "Direct connection",
            "TLS 1.3",
            "Pinned identity",
            "Local network",
        )) {
            assertTrue("the footer dropped \"$fact\"", exchange.contains("\"$fact\""))
        }
    }

    @Test
    fun `the security footer claims nothing the implementation does not do`() {
        val overclaims = listOf(
            "end-to-end",
            "end to end",
            "zero knowledge",
            "zero-knowledge",
            "military",
            "unbreakable",
            "anonymous",
        )
        for (name in exchangeScreens) {
            val text = code(name)
            for (claim in overclaims) {
                assertFalse(
                    "$name claims \"$claim\", which is more than the link does",
                    text.contains(claim, ignoreCase = true),
                )
            }
        }
    }

    /** It is announced as one sentence, not as four unrelated fragments. */
    @Test
    fun `the security footer is announced as one sentence`() {
        val exchange = source("ui/components/Exchange.kt")
        assertTrue(
            "the footer must merge its descendants and announce one description",
            exchange.contains("semantics(mergeDescendants = true)") &&
                exchange.contains("Secure connection: direct connection"),
        )
    }

    // ---- the artwork never carries a fact --------------------------------

    /**
     * The hero is hidden from assistive technology.
     *
     * It is a picture of what the screen is doing, and every fact in it —
     * the payload, the destination, the state of the link — is said in words
     * by the pill and the payload card below. Artwork that announced itself
     * would read the same thing twice; artwork that was the *only* place a
     * fact appeared would be unreadable to anyone not looking at it.
     */
    @Test
    fun `the exchange hero is decorative and says nothing`() {
        val exchange = source("ui/components/Exchange.kt")
        val hero = exchange.substringAfter("fun ExchangeHero(").substringBefore("\n}")
        assertTrue("the hero must be hidden from assistive technology", hero.contains(".decorative()"))
    }

    /** The peer pill announces the destination and its identity together. */
    @Test
    fun `the peer pill announces its name and identity as one object`() {
        val exchange = source("ui/components/Exchange.kt")
        val pill = exchange.substringAfter("fun ExchangePeerStatus(").substringBefore("\n}\n")
        assertTrue(
            "the pill must merge its two lines into one announcement",
            pill.contains("mergeDescendants = true"),
        )
        assertTrue(
            "the pill must announce the live label and the live identity",
            pill.contains("\"\$label. \$identity\""),
        )
    }

    /**
     * The connected tint is the semantic status colour, not a brand hue.
     *
     * Teal means connected in this app. A pill that reached for
     * `Brand.Cyan` because the reference looks green would be painting a
     * status with a decoration colour, and the two are allowed to differ.
     */
    @Test
    fun `the peer pill takes its colour from the status vocabulary`() {
        val exchange = source("ui/components/Exchange.kt")
        val pill = exchange.substringAfter("fun ExchangePeerStatus(").substringBefore("\n}\n")
        assertTrue("the pill must colour itself from the status", pill.contains("status.color()"))
        assertTrue("the dot must come from the status too", pill.contains("status.dot()"))
    }

    // ---- the rejected decoration is gone ---------------------------------

    /**
     * The device card no longer wears the decorative stroke.
     *
     * It was drawn only for a connected device and said nothing the badge did
     * not already say in a word. Nothing replaced it: the card was not short
     * of content, only of decoration.
     */
    @Test
    fun `the device card carries no decorative flourish`() {
        val cards = source("ui/components/Cards.kt")
        assertFalse(
            "the ribbon flourish is back in the device card",
            cards.contains("PliweeRibbonFlourish"),
        )
        val sources = File("src/main/java").walkTopDown()
            .filter { it.isFile && it.extension == "kt" }
        for (file in sources) {
            assertFalse(
                "${file.name} still draws PliweeRibbonFlourish",
                file.readText().contains("PliweeRibbonFlourish"),
            )
        }
    }

    /**
     * The Files list's empty state names what is missing.
     *
     * It used to be the connection ribbon: a thin tricolour stroke above the
     * heading that read as a stray underline and said nothing about files.
     */
    @Test
    fun `the files empty state uses a files mark`() {
        val files = source("ui/FilesScreen.kt")
        val empty = files.substringAfter("PliweeEmptyState(")
            .substringBefore("fillsContentArea")
        assertTrue(
            "the Files empty state must name its own subject",
            empty.contains("R.drawable.ic_files"),
        )
        assertFalse(
            "the Files empty state must not draw the ribbon",
            withoutComments(empty).contains("ribbon"),
        )
    }

    /**
     * Every exchange empty state uses a semantic glyph.
     *
     * The rejected motif must not come back as a different squiggle either:
     * the assertion is that the payload empty state draws a named icon
     * resource, which a decorative path cannot satisfy.
     */
    @Test
    fun `the payload empty state draws a named icon`() {
        val exchange = source("ui/components/Exchange.kt")
        val empty = exchange.substringAfter("fun ExchangePayloadEmpty(").substringBefore("\n}\n")
        assertTrue(
            "the empty payload must draw the icon it was handed",
            empty.contains("PliweeEmptyArt(icon = icon"),
        )
        assertFalse("no ribbon in an exchange empty state", empty.contains("ribbon"))
    }

    // ---- the flow graphic is drawn, and is a flow -------------------------

    /**
     * The flow is two ribbons travelling the same way, drawn natively.
     *
     * The single thin wave it replaced read as an underline. Two paths, both
     * running left to right and both rising, are what make it read as
     * movement in a direction rather than as a squiggle — and it is a Canvas
     * path rather than a bitmap, so it scales to any density.
     */
    @Test
    fun `the flow graphic is two native paths and no raster`() {
        val exchange = source("ui/components/Exchange.kt")
        val flow = exchange.substringAfter("fun ExchangeFlow(").substringBefore("\n}\n")
        assertEquals(
            "the flow must be exactly two ribbons",
            2,
            Regex("""\bdrawPath\(""").findAll(flow).count(),
        )
        assertTrue("the flow must be drawn, not rasterised", flow.contains("Canvas("))
        for (raster in listOf("painterResource", ".png", "ImageBitmap")) {
            assertFalse("the flow must not load $raster", flow.contains(raster))
        }
    }

    /**
     * Neither ribbon ever travels backwards.
     *
     * This is the property that keeps the picture meaning "from here to
     * there". Every control point and end point of both paths is read in
     * order and must be weakly increasing in x: a path that turned back would
     * draw a loop, a crossing or a squiggle, which is exactly what the single
     * thin wave was replaced for.
     */
    @Test
    fun `neither ribbon ever travels backwards`() {
        val flow = source("ui/components/Exchange.kt")
            .substringAfter("fun ExchangeFlow(").substringBefore("\n}\n")
        for (name in listOf("lead", "trail")) {
            val body = flow.substringAfter("val $name = Path().apply {")
                .substringBefore("}")
            val xs = Regex("""w \* (0\.\d+)f""").findAll(body)
                .map { it.groupValues[1].toFloat() }.toList()
            assertTrue("$name has too few points to be a curve", xs.size >= 7)
            assertEquals(
                "$name must run strictly left to right",
                xs.sorted(),
                xs,
            )
        }
    }

    /**
     * Between them the ribbons span the whole gap.
     *
     * The near one starts at the source tile and the far one finishes at the
     * destination tile, which is what makes the two of them read as one
     * bridge rather than as two unrelated strokes floating in the middle.
     */
    @Test
    fun `the ribbons span the gap from source to destination`() {
        val flow = source("ui/components/Exchange.kt")
            .substringAfter("fun ExchangeFlow(").substringBefore("\n}\n")
        fun xs(name: String): List<Float> =
            Regex("""w \* (0\.\d+)f""")
                .findAll(flow.substringAfter("val $name = Path().apply {").substringBefore("}"))
                .map { it.groupValues[1].toFloat() }.toList()

        assertTrue("the near ribbon must start at the source", xs("lead").first() < 0.10f)
        assertTrue("the far ribbon must reach the destination", xs("trail").last() > 0.90f)
    }

    /**
     * The flow has both of its terminals.
     *
     * Four circles: a terminal at each end of the bridge and two motes
     * between them. Drop the terminals and the ribbons fray out into nothing
     * at both ends, which is how the first attempt came to read as a stray
     * pen stroke rather than as something travelling between two points.
     */
    @Test
    fun `the flow keeps its two terminals and two motes`() {
        val flow = source("ui/components/Exchange.kt")
            .substringAfter("fun ExchangeFlow(").substringBefore("\n}\n")
        assertEquals(
            "the flow must carry exactly four motes",
            4,
            Regex("""\bdrawCircle\(""").findAll(flow).count(),
        )
        val lead = flow.substringAfter("val lead = Path().apply {").substringBefore("}")
        val trail = flow.substringAfter("val trail = Path().apply {").substringBefore("}")
        val leadStart = Regex("""moveTo\(w \* (0\.\d+)f, h \* (0\.\d+)f\)""")
            .find(lead)!!.groupValues.drop(1)
        assertTrue(
            "a terminal must sit on the near ribbon's start",
            flow.contains("Offset(w * ${leadStart[0]}f, h * ${leadStart[1]}f)"),
        )
        val trailEnd = Regex("""w \* (0\.\d+)f, h \* (0\.\d+)f\)$""",
            RegexOption.MULTILINE).findAll(trail).last().groupValues.drop(1)
        assertTrue(
            "a terminal must sit on the far ribbon's end",
            flow.contains("Offset(w * ${trailEnd[0]}f, h * ${trailEnd[1]}f)"),
        )
    }

    /**
     * The hero is the same composition on both exchange screens.
     *
     * Two screens drawing their own version of one motif is how they drift;
     * the shared component is the whole point of the family.
     */
    @Test
    fun `both exchange screens use the shared hero and footer`() {
        for (name in listOf("ui/SendClipboardScreen.kt", "ui/SendActivity.kt")) {
            val text = source(name)
            assertTrue("$name must use the shared hero", text.contains("ExchangeHero("))
            assertTrue(
                "$name must use the shared security footer",
                text.contains("ExchangeSecurityFooter()"),
            )
            assertTrue(
                "$name must use the shared payload card",
                text.contains("ExchangePayloadCard("),
            )
        }
    }

    /**
     * The exchange language stays on the exchange screens.
     *
     * It is a vocabulary for sending one thing to one computer, not a new
     * global layout. A Devices, Settings or notification screen wearing the
     * hero would be claiming to be a send.
     */
    @Test
    fun `the exchange hero stays off the screens that are not exchanges`() {
        val elsewhere = listOf(
            "ui/DevicesScreen.kt",
            "ui/SettingsScreen.kt",
            "ui/NotificationSettingsScreen.kt",
            "ui/FilesScreen.kt",
            "ui/PeerDetailScreen.kt",
            "ui/AppPickerScreen.kt",
        )
        for (name in elsewhere) {
            assertFalse(
                "$name is not an exchange flow and must not wear the hero",
                source(name).contains("ExchangeHero("),
            )
        }
    }

    // ---- the tablet keeps its margins -------------------------------------

    /**
     * Both exchange screens cap their content column.
     *
     * The Sharesheet screen is outside the app shell, so it does not inherit
     * the shell's cap and has to ask for it. Without it the payload card
     * becomes a 1300dp band on the tablet in landscape.
     */
    @Test
    fun `both exchange screens keep the content column cap`() {
        for (name in listOf("ui/SendClipboardScreen.kt", "ui/SendActivity.kt")) {
            assertTrue(
                "$name must cap and centre its column",
                source(name).contains("pliweeContentColumn()"),
            )
        }
    }

    /**
     * The disabled primary action is not the brand gradient.
     *
     * A translucent gradient reads as "still tappable, just pretty". The
     * button draws a flat neutral instead, and that is a property of the one
     * component both screens use.
     */
    @Test
    fun `the disabled primary action is neutral rather than branded`() {
        val buttons = source("ui/components/Buttons.kt")
        val primary = buttons.substringAfter("fun PliweePrimaryButton(")
            .substringBefore("\n}\n")
        assertTrue(
            "the enabled button wears the CTA gradient",
            primary.contains("PliweeGradient.cta()"),
        )
        assertTrue(
            "the disabled button must be a flat neutral",
            primary.contains("listOf(colors.surfaceSunken, colors.surfaceSunken)"),
        )
        assertTrue(
            "the button keeps the minimum touch target",
            primary.contains("heightIn(min = MinTouchTarget)"),
        )
    }
}
