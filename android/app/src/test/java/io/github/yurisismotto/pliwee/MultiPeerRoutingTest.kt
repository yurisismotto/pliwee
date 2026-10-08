package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.net.Backoff
import io.github.yurisismotto.pliwee.net.ConnectionCoordinator
import io.github.yurisismotto.pliwee.net.ConnectionEvent
import io.github.yurisismotto.pliwee.net.DialResult
import io.github.yurisismotto.pliwee.net.Endpoints
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.PeerTarget
import io.github.yurisismotto.pliwee.store.TrustStore
import java.net.InetSocketAddress
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Where connection attempts actually go, on a virtual clock.
 *
 * ## Why this is not [PeerTargetTest]
 *
 * That suite proves the *policy* is order-independent. This one proves the
 * policy is what the running connection loop obeys — that the target is
 * re-read every round, that choosing a peer mid-backoff takes effect, and that
 * a peer coming back online cannot take a session away from the chosen one.
 *
 * The wiring below is deliberately the same composition `ConnectionService`
 * uses: `PeerTarget.resolve` decides the peer, the peer's addresses become the
 * coordinator's endpoints, and the dial is attempted against a fake network
 * that says which desktops are powered on. A test that stubbed the target out
 * would prove nothing about the defect, which lived exactly in that seam.
 *
 * Every test drives *time*, never events, for the reason [ReconnectTest]
 * does: anything that needed a network callback to make progress would pass
 * with the original bug present.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MultiPeerRoutingTest {

    // -- the world ----------------------------------------------------------

    private fun fingerprint(seed: Int) = Fingerprint(ByteArray(32) { (seed + it).toByte() })

    private fun peer(name: String, seed: Int) = TrustStore.TrustedPeer(
        deviceId = "device-$seed",
        deviceName = name,
        fingerprint = fingerprint(seed),
        pairedAtUnix = 1_700_000_000L + seed,
        grantedCapabilities = emptySet(),
        addressHints = listOf(AddressHint("192.168.68.${70 + seed}", 55432)),
    )

    private val ubuntu = peer("omnibridge-u2404", 1)
    private val debian = peer("omnibridge-d13", 2)
    private val ubuntu26 = peer("omnibridge-u2604", 3)

    private fun hex(p: TrustStore.TrustedPeer) = p.fingerprint.toHex()

    /**
     * A tablet: a trust store, a choice, and a view of which desktops are on.
     *
     * Mutable, because the scenarios are about state changing underneath a
     * running loop — a desktop booting, a session dropping, a person choosing
     * a different computer.
     */
    private class Tablet(
        @Volatile var peers: List<TrustStore.TrustedPeer>,
        @Volatile var selectedHex: String? = null,
        @Volatile var online: Set<String> = emptySet(),
    ) {
        /** Every address dialled, in order, with the peer it belonged to. */
        val dialled = CopyOnWriteArrayList<String>()

        /** Which peers a session was actually established with. */
        val sessions = CopyOnWriteArrayList<String>()

        fun online(vararg peers: TrustStore.TrustedPeer) {
            online = peers.map { it.deviceName }.toSet()
        }

        fun ownerOf(address: InetSocketAddress): TrustStore.TrustedPeer? =
            peers.firstOrNull { p ->
                p.addressHints.any { it.format() == Endpoints.format(address) }
            }
    }

    private fun TestScope.coordinatorFor(
        tablet: Tablet,
        events: MutableList<ConnectionEvent> = CopyOnWriteArrayList(),
        session: suspend (TrustStore.TrustedPeer) -> String = { "peer closed" },
    ): ConnectionCoordinator = ConnectionCoordinator(
        scope = this,
        // Exactly `ConnectionService.endpointsFor`: resolve the target, then
        // ask for its addresses. Nothing here can see list position.
        endpoints = {
            PeerTarget.resolve(tablet.peers, tablet.selectedHex)
                .peerOrNull()
                ?.dialAddresses()
                ?: emptyList()
        },
        // Exactly `ConnectionService.dial`: re-resolve, refuse when there is no
        // unambiguous target, otherwise attempt the socket.
        dial = { address ->
            val resolution = PeerTarget.resolve(tablet.peers, tablet.selectedHex)
            val target = resolution.peerOrNull()
            if (target == null) {
                DialResult.Terminal(resolution.blockedReason() ?: "no paired computer")
            } else {
                tablet.dialled += target.deviceName
                val owner = tablet.ownerOf(address)
                when {
                    // The pin is checked against the *target*, so an address
                    // that belongs to another computer cannot become a session
                    // with it. This is what keeps discovery from being trust.
                    owner == null || owner.deviceName != target.deviceName ->
                        DialResult.Security("not the pinned identity")

                    target.deviceName !in tablet.online ->
                        DialResult.Transient("connection refused")

                    else -> DialResult.Established {
                        tablet.sessions += target.deviceName
                        session(target)
                    }
                }
            }
        },
        backoff = Backoff(random = { 0.5 }),
        log = { events += it },
        // Exactly `ConnectionService`'s wiring: an ambiguity or an empty trust
        // store stops the loop with its own reason instead of retrying.
        blocked = { PeerTarget.resolve(tablet.peers, tablet.selectedHex).blockedReason() },
    )

    /**
     * Long enough for several rounds of the 2s-base transient ladder.
     *
     * `advanceTimeBy` and deliberately **not** `advanceUntilIdle`: a healthy
     * reconnect loop is never idle, so waiting for idleness against a peer
     * that is refusing connections would spin the virtual clock forever. The
     * same note guards [ReconnectTest].
     */
    private fun TestScope.settle(millis: Long = 30_000) {
        advanceTimeBy(millis)
    }

    // -- R1  the certified reproduction, as a running loop -------------------

    @Test
    fun `R1 an offline first trusted peer does not absorb attempts meant for a later one`() =
        runTest {
            // The U2 §39.17 trust store, in its captured order.
            val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = hex(debian))
            tablet.online(debian)

            val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
            c.start()
            settle()

            assertEquals(
                "only the chosen computer is ever dialled",
                setOf("omnibridge-d13"),
                tablet.dialled.toSet(),
            )
            assertEquals(listOf("omnibridge-d13"), tablet.sessions.toList())
            assertFalse(
                "the powered-off first entry must receive nothing",
                tablet.dialled.contains("omnibridge-u2404"),
            )
            c.stop()
        }

    @Test
    fun `R2 the same holds with the trust store order reversed`() = runTest {
        val tablet = Tablet(peers = listOf(debian, ubuntu), selectedHex = hex(debian))
        tablet.online(debian)

        val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
        c.start()
        settle()

        assertEquals(setOf("omnibridge-d13"), tablet.dialled.toSet())
        assertEquals(listOf("omnibridge-d13"), tablet.sessions.toList())
        c.stop()
    }

    @Test
    fun `R3 selecting the offline peer dials it and only it`() = runTest {
        // The mirror: the fix must not have become "prefer whichever peer is
        // reachable". An explicit choice is obeyed even when it fails.
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = hex(ubuntu))
        tablet.online(debian)

        val c = coordinatorFor(tablet)
        c.start()
        settle()

        assertEquals(setOf("omnibridge-u2404"), tablet.dialled.toSet())
        assertTrue("no session with an unchosen peer", tablet.sessions.isEmpty())
        c.stop()
    }

    // -- R4  two reachable peers --------------------------------------------

    @Test
    fun `R4 with both desktops online the chosen one receives the connection`() = runTest {
        for (order in listOf(listOf(ubuntu, debian), listOf(debian, ubuntu))) {
            for (wanted in listOf(ubuntu, debian)) {
                val tablet = Tablet(peers = order, selectedHex = hex(wanted))
                tablet.online(ubuntu, debian)

                val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
                c.start()
                settle()

                assertEquals(
                    "order ${order.map { it.deviceName }} choosing ${wanted.deviceName}",
                    listOf(wanted.deviceName),
                    tablet.sessions.toList(),
                )
                c.stop()
            }
        }
    }

    @Test
    fun `R5 with three trusted peers only the chosen one is contacted`() = runTest {
        val tablet = Tablet(
            peers = listOf(ubuntu, debian, ubuntu26),
            selectedHex = hex(ubuntu26),
        )
        // Ubuntu offline, the other two reachable: a "first reachable peer"
        // implementation would settle on Debian and pass every earlier test.
        tablet.online(debian, ubuntu26)

        val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
        c.start()
        settle()

        assertEquals(setOf("omnibridge-u2604"), tablet.dialled.toSet())
        assertEquals(listOf("omnibridge-u2604"), tablet.sessions.toList())
        c.stop()
    }

    // -- R6  no ambiguity is ever resolved silently --------------------------

    @Test
    fun `R6 two trusted peers and no choice dials nothing at all`() = runTest {
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = null)
        tablet.online(ubuntu, debian)

        val events = CopyOnWriteArrayList<ConnectionEvent>()
        val c = coordinatorFor(tablet, events)
        c.start()
        settle()

        assertTrue("nothing may be dialled on a guess", tablet.dialled.isEmpty())
        assertTrue(tablet.sessions.isEmpty())
        // And it says so rather than retrying forever behind a countdown.
        assertTrue(
            events.any {
                it.kind == ConnectionEvent.Kind.STOPPED &&
                    it.fields.any { f -> f.second == "choose which computer to connect to" }
            },
        )
        c.stop()
    }

    // -- R7  retargeting a running loop --------------------------------------

    @Test
    fun `R7 choosing a reachable peer while looping on an offline one takes effect`() = runTest {
        // The certified scenario end to end: Ubuntu chosen and off, the loop
        // backing off against it, then the person picks Debian.
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = hex(ubuntu))
        tablet.online(debian)

        val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
        c.start()
        settle(20_000)
        assertTrue("backing off against Ubuntu", tablet.dialled.isNotEmpty())
        assertTrue(tablet.sessions.isEmpty())

        val beforeChoice = tablet.dialled.size
        tablet.selectedHex = hex(debian)
        c.onTargetChanged(debian.fingerprint.toDisplayShort())
        settle(5_000)

        assertEquals(listOf("omnibridge-d13"), tablet.sessions.toList())
        assertTrue(
            "the retry must not have waited out Ubuntu's backoff",
            tablet.dialled.drop(beforeChoice).contains("omnibridge-d13"),
        )
        c.stop()
    }

    @Test
    fun `R8 a peer coming online later does not hijack the chosen session`() = runTest {
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = hex(debian))
        tablet.online(debian)

        val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
        c.start()
        settle()
        assertEquals(listOf("omnibridge-d13"), tablet.sessions.toList())

        // Ubuntu boots. It is trusted, it is first in the store, and it is now
        // reachable — every condition the old code needed to steal the link.
        tablet.online(ubuntu, debian)
        c.onNetworkAvailable("wifi")
        settle()

        assertEquals("the session stays with Debian", listOf("omnibridge-d13"), tablet.sessions.toList())
        assertFalse(tablet.dialled.contains("omnibridge-u2404"))
        c.stop()
    }

    // -- R9  reconnect -------------------------------------------------------

    @Test
    fun `R9 a dropped session reconnects to the same chosen peer`() = runTest {
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = hex(debian))
        // Both reachable, so a reconnect has somewhere wrong to go.
        tablet.online(ubuntu, debian)

        var drops = 0
        val c = coordinatorFor(tablet) {
            if (drops++ == 0) "network dropped" else CompletableDeferred<String>().await()
        }
        c.start()
        settle()

        assertEquals(
            "both the first session and the reconnect are Debian",
            listOf("omnibridge-d13", "omnibridge-d13"),
            tablet.sessions.toList(),
        )
        assertFalse(tablet.dialled.contains("omnibridge-u2404"))
        c.stop()
    }

    @Test
    fun `R10 changing the choice after a disconnect connects to the new peer`() = runTest {
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = hex(debian))
        tablet.online(ubuntu, debian)

        val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
        c.start()
        settle()
        assertEquals(listOf("omnibridge-d13"), tablet.sessions.toList())

        // Disconnect, choose Ubuntu, connect again — a fresh coordinator, the
        // way the service builds one after `stopSelf`.
        c.stop()
        tablet.selectedHex = hex(ubuntu)
        val c2 = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
        c2.start()
        settle()

        assertEquals(listOf("omnibridge-d13", "omnibridge-u2404"), tablet.sessions.toList())
        c2.stop()
    }

    @Test
    fun `R11 a restart keeps the persisted choice, and nothing else decides`() = runTest {
        // What survives an app restart is the hex in the trust store. Rebuilt
        // from that alone, the loop must reach the same computer — including
        // when the offline peer is the one the old code would have picked.
        val persisted = hex(debian)
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = persisted)
        tablet.online(debian)

        val c = coordinatorFor(tablet) { CompletableDeferred<String>().await() }
        c.start()
        settle()

        assertEquals(listOf("omnibridge-d13"), tablet.sessions.toList())
        c.stop()
    }

    // -- R12  security -------------------------------------------------------

    @Test
    fun `R12 an address belonging to another computer never becomes a session`() = runTest {
        // Debian is chosen, but its remembered address now answers as Ubuntu —
        // a rebound DHCP lease, or a hostile responder. The pin is checked
        // against the target, so this is a security failure, not a session.
        val movedDebian = debian.copy(addressHints = ubuntu.addressHints)
        val tablet = Tablet(peers = listOf(ubuntu, movedDebian), selectedHex = hex(movedDebian))
        tablet.online(ubuntu, movedDebian)

        val events = CopyOnWriteArrayList<ConnectionEvent>()
        val c = coordinatorFor(tablet, events)
        c.start()
        settle()

        assertTrue("no session with a mismatched identity", tablet.sessions.isEmpty())
        assertTrue(
            events.any { e ->
                e.kind == ConnectionEvent.Kind.CONNECT_FAILURE &&
                    e.fields.any { it == ("kind" to "security") }
            },
        )
        c.stop()
    }

    @Test
    fun `R13 the retarget event names the peer by fingerprint prefix, not by content`() = runTest {
        val tablet = Tablet(peers = listOf(ubuntu, debian), selectedHex = hex(ubuntu))
        val events = CopyOnWriteArrayList<ConnectionEvent>()
        val c = coordinatorFor(tablet, events)
        c.start()
        settle(5_000)

        c.onTargetChanged(debian.fingerprint.toDisplayShort())
        settle(1_000)

        val retarget = events.first { it.kind == ConnectionEvent.Kind.TARGET_CHANGED }
        assertEquals(
            listOf("peer" to debian.fingerprint.toDisplayShort()),
            retarget.fields,
        )
        // A display-short fingerprint and nothing else: no device name, no
        // address, and certainly no payload.
        assertFalse(retarget.toString().contains("omnibridge-d13"))
        c.stop()
    }
}
