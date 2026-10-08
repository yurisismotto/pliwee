package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.capability.NotificationsCapability
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.net.Endpoints
import io.github.yurisismotto.pliwee.notifications.LockPolicy
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.TrustStore
import java.io.File
import java.net.InetSocketAddress
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Remembered peer addresses, migrated to Mesh V2 address hints.
 *
 * ## What is being protected
 *
 * Every install before this one stored `addresses: [String]` on each peer
 * record. Mesh V2 SPEC §5 replaces that with bounded, structured hints, and
 * §11 fixes what the migration may not do: change the identity key, the peer
 * fingerprint, the device id, the revoked state or a grant. A migration that
 * got any of those wrong would cost a person their pairings — or, worse, keep
 * a pairing that should have gone.
 *
 * So the fixture below is a file exactly as the previous build wrote it, and
 * every assertion about identity, grants and policy is made against it.
 */
class AddressHintMigrationTest {

    // -- the file an existing install has --------------------------------------

    /**
     * `trust-store.json` as written by the build before this change: two
     * trusted computers, one revoked, one tombstone. Byte-for-byte the shape
     * `TrustStore.writePeers` produced — `addresses` strings, no hints.
     */
    private val currentFile = """
        {
          "schemaVersion": 1,
          "deviceId": "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
          "deviceName": "Pixel 8",
          "selectedPeer": "${Fixtures.IDENTITY_A_FINGERPRINT}",
          "notificationSecretGeneration": 1,
          "peers": [
            {
              "deviceId": "795fec0868ebef8c3d7ad3e775dc6ed0",
              "deviceName": "omnibridge-f42",
              "fingerprint": "${Fixtures.IDENTITY_A_FINGERPRINT}",
              "pairedAtUnix": 1700000000,
              "grantedCapabilities": ["files.v1", "battery.v1", "clipboard.v1", "notifications.v1"],
              "addresses": ["192.168.68.72:55432", "[fe80::1%wlan0]:55432", "fedora.local:55432"],
              "clipboardPolicy": {"allowSend": true, "allowReceive": false, "autoSend": false, "autoReceive": true},
              "notificationPolicy": {
                "allowMirror": true,
                "allowedApps": ["com.example.chat"],
                "knownApps": ["com.example.bank", "com.example.chat"],
                "includeWorkProfile": false,
                "includeOngoing": false,
                "whenSourceLocked": "APP_ONLY",
                "allowDismissSync": true
              }
            },
            {
              "deviceId": "d13d13d13d13d13d13d13d13d13d13d1",
              "deviceName": "omnibridge-d13",
              "fingerprint": "$DEBIAN_HEX",
              "pairedAtUnix": 1700000500,
              "grantedCapabilities": ["files.v1"],
              "addresses": [],
              "clipboardPolicy": {"allowSend": true, "allowReceive": true, "autoSend": false, "autoReceive": false},
              "notificationPolicy": {"allowMirror": true, "allowedApps": [], "knownApps": []}
            },
            {
              "deviceId": "revokedrevokedrevokedrevokedrevo",
              "deviceName": "old-laptop",
              "fingerprint": "$REVOKED_HEX",
              "pairedAtUnix": 1600000000,
              "grantedCapabilities": [],
              "addresses": ["192.168.68.50:55432"],
              "revoked": true,
              "clipboardPolicy": {"allowSend": false, "allowReceive": false, "autoSend": false, "autoReceive": false},
              "notificationPolicy": {"allowMirror": false, "allowedApps": [], "knownApps": []}
            },
            {
              "deviceId": "",
              "deviceName": "",
              "fingerprint": "$TOMBSTONE_HEX",
              "pairedAtUnix": 0,
              "grantedCapabilities": [],
              "addresses": ["192.168.68.51:55432"],
              "revoked": true,
              "hiddenFromUi": true,
              "clipboardPolicy": {"allowSend": false, "allowReceive": false, "autoSend": false, "autoReceive": false},
              "notificationPolicy": {"allowMirror": false, "allowedApps": [], "knownApps": []}
            }
          ]
        }
    """.trimIndent()

    private fun file() = JSONObject(currentFile)

    private fun entries(): List<JSONObject> = file().getJSONArray("peers").let { array ->
        (0 until array.length()).map { array.getJSONObject(it) }
    }

    private fun records(): List<TrustStore.TrustedPeer> =
        entries().map { checkNotNull(TrustStore.TrustedPeer.fromJson(it)) }

    private fun record(hex: String) = records().single { it.fingerprint.toHex() == hex }

    // -- identity, grants and policy are untouched -------------------------------

    @Test
    fun `the schema version does not move, so the file is kept rather than replaced`() {
        // `TrustStore.load` replaces a file whose version it does not match
        // with a fresh store. Moving the version would be the one way this
        // change could cost every install its pairings.
        assertEquals(1, TrustStore.SCHEMA_VERSION)
        assertEquals(TrustStore.SCHEMA_VERSION, file().getInt("schemaVersion"))
    }

    @Test
    fun `migration keeps every fingerprint, device id, grant and policy`() {
        val entries = entries()
        val migrated = records()
        assertEquals("no record is dropped", entries.size, migrated.size)

        for ((entry, peer) in entries.zip(migrated)) {
            assertEquals(entry.getString("fingerprint"), peer.fingerprint.toHex())
            assertEquals(entry.getString("deviceId"), peer.deviceId)
            assertEquals(entry.getString("deviceName"), peer.deviceName)
            assertEquals(entry.getLong("pairedAtUnix"), peer.pairedAtUnix)
            val grants = entry.getJSONArray("grantedCapabilities")
            assertEquals(
                (0 until grants.length()).map { grants.getString(it) }.toSet(),
                peer.grantedCapabilities,
            )
            assertEquals(
                ClipboardPolicy.fromJson(entry.getJSONObject("clipboardPolicy")),
                peer.clipboardPolicy,
            )
            assertEquals(
                NotificationPolicy.fromJson(entry.getJSONObject("notificationPolicy")),
                peer.notificationPolicy,
            )
            assertEquals(
                entry.optBoolean("revoked", false) || entry.optBoolean("hiddenFromUi", false),
                peer.revoked,
            )
            assertEquals(entry.optBoolean("hiddenFromUi", false), peer.hidden)
        }
    }

    @Test
    fun `the person's own decisions read back exactly`() {
        val fedora = record(Fixtures.IDENTITY_A_FINGERPRINT)
        assertTrue(fedora.allows(ClipboardCapability.ID))
        assertTrue(fedora.allows(NotificationsCapability.ID))
        assertEquals(
            ClipboardPolicy(allowSend = true, allowReceive = false, autoReceive = true),
            fedora.clipboardPolicy,
        )
        assertEquals(setOf("com.example.chat"), fedora.notificationPolicy.allowedApps)
        assertEquals(LockPolicy.APP_ONLY, fedora.notificationPolicy.whenSourceLocked)
        assertEquals(
            "the trusted set is the trusted set it was",
            listOf(Fixtures.IDENTITY_A_FINGERPRINT, DEBIAN_HEX),
            TrustStore.trustedOf(records()).map { it.fingerprint.toHex() },
        )
    }

    @Test
    fun `migration and a write leave identity, grants and policy unchanged`() {
        for (peer in records()) {
            val again = checkNotNull(TrustStore.TrustedPeer.fromJson(peer.toJson()))
            assertEquals(peer.fingerprint.toHex(), again.fingerprint.toHex())
            assertEquals(peer.deviceId, again.deviceId)
            assertEquals(peer.grantedCapabilities, again.grantedCapabilities)
            assertEquals(peer.clipboardPolicy, again.clipboardPolicy)
            assertEquals(peer.notificationPolicy, again.notificationPolicy)
            assertEquals(peer.revoked, again.revoked)
            assertEquals(peer.hidden, again.hidden)
            assertEquals(peer.addressHints, again.addressHints)
        }
    }

    // -- existing addresses become bounded hints --------------------------------

    @Test
    fun `existing addresses become hints in their stored order with no known success time`() {
        val fedora = record(Fixtures.IDENTITY_A_FINGERPRINT)
        assertEquals(
            listOf(
                AddressHint("192.168.68.72", 55432, 0),
                AddressHint("fe80::1%wlan0", 55432, 0),
                AddressHint("fedora.local", 55432, 0),
            ),
            fedora.addressHints,
        )
        assertEquals(
            "and dial exactly where the old strings pointed",
            listOf("192.168.68.72:55432", "[fe80::1%wlan0]:55432", "fedora.local:55432"),
            fedora.dialAddresses().map(Endpoints::format),
        )
        assertEquals(emptyList<AddressHint>(), record(DEBIAN_HEX).addressHints)
    }

    @Test
    fun `a migrated record is written with hints and with the old strings an older build reads`() {
        val json = record(Fixtures.IDENTITY_A_FINGERPRINT).toJson()
        val legacy = json.getJSONArray("addresses")
        assertEquals(
            listOf("192.168.68.72:55432", "[fe80::1%wlan0]:55432", "fedora.local:55432"),
            (0 until legacy.length()).map { legacy.getString(it) },
        )
        val hints = json.getJSONArray("addressHints")
        assertEquals(3, hints.length())
        assertEquals("192.168.68.72", hints.getJSONObject(0).getString("host"))
        assertEquals(55432, hints.getJSONObject(0).getInt("port"))
        assertEquals(0L, hints.getJSONObject(0).getLong("lastSuccessUnix"))
    }

    @Test
    fun `a record with nothing to remember is written as an older build wrote it`() {
        val json = record(DEBIAN_HEX).toJson()
        assertFalse(json.has("addressHints"))
        assertEquals(0, json.getJSONArray("addresses").length())
    }

    @Test
    fun `structured hints win over the legacy strings beside them`() {
        val entry = entries()[0].apply {
            put(
                "addressHints",
                JSONArray().put(AddressHint("10.0.0.9", 55432, 1_800_000_000L).toJson()),
            )
        }
        val peer = checkNotNull(TrustStore.TrustedPeer.fromJson(entry))
        assertEquals(listOf(AddressHint("10.0.0.9", 55432, 1_800_000_000L)), peer.addressHints)
    }

    @Test
    fun `more than eight addresses is reduced deterministically to the first eight`() {
        val stored = (1..12).map { "192.168.68.$it:55432" }
        val entry = entries()[0].put("addresses", JSONArray(stored))

        val first = checkNotNull(TrustStore.TrustedPeer.fromJson(entry)).addressHints
        val second = checkNotNull(TrustStore.TrustedPeer.fromJson(entry)).addressHints

        assertEquals(8, first.size)
        assertEquals(stored.take(8), first.map { it.format() })
        assertEquals("the same file reduces the same way every time", first, second)
    }

    @Test
    fun `more than eight structured hints is reduced the same way`() {
        val hints = JSONArray()
        (1..10).forEach { hints.put(AddressHint("10.0.0.$it", 55432, 1_800_000_000L + it).toJson()) }
        val entry = entries()[0].put("addressHints", hints)
        val peer = checkNotNull(TrustStore.TrustedPeer.fromJson(entry))
        assertEquals((1..8).map { "10.0.0.$it" }, peer.addressHints.map { it.host })
    }

    @Test
    fun `duplicate and equivalent addresses collapse into the first, most recent, one`() {
        val entry = entries()[0].put(
            "addresses",
            JSONArray(
                listOf(
                    "[FE80:0:0::0001%wlan0]:55432",
                    "192.168.68.72:55432",
                    "[fe80::1%wlan0]:55432",
                    " 192.168.68.72:55432",
                    "Fedora.Local.:55432",
                    "fedora.local:55432",
                    "192.168.68.72:55433",
                ),
            ),
        )
        val peer = checkNotNull(TrustStore.TrustedPeer.fromJson(entry))
        assertEquals(
            listOf(
                "[fe80::1%wlan0]:55432",
                "192.168.68.72:55432",
                "fedora.local:55432",
                // A different port is a different endpoint.
                "192.168.68.72:55433",
            ),
            peer.addressHints.map { it.format() },
        )
    }

    @Test
    fun `an address that names no endpoint is dropped and nothing else is touched`() {
        val entry = entries()[0].put(
            "addresses",
            JSONArray(
                listOf(
                    "garbage",
                    "10.0.0.1:0",
                    "10.0.0.1:70000",
                    "[zz::1]:55432",
                    "010.0.0.1:55432",
                    "10.0.0.2:55432",
                ),
            ),
        )
        val peer = checkNotNull(TrustStore.TrustedPeer.fromJson(entry))
        assertEquals(listOf("10.0.0.2:55432"), peer.addressHints.map { it.format() })
        assertEquals(Fixtures.IDENTITY_A_FINGERPRINT, peer.fingerprint.toHex())
        assertTrue(peer.allows(NotificationsCapability.ID))
    }

    @Test
    fun `a malformed hints array costs the hints and nothing else`() {
        val entry = entries()[0].put(
            "addressHints",
            JSONArray().put("not an object").put(JSONObject().put("host", "").put("port", 1)),
        )
        val peer = checkNotNull(TrustStore.TrustedPeer.fromJson(entry))
        assertEquals(emptyList<AddressHint>(), peer.addressHints)
        assertEquals("795fec0868ebef8c3d7ad3e775dc6ed0", peer.deviceId)
        assertTrue(peer.allows(FilesCapability.ID))
    }

    @Test
    fun `a hint never rescues a record with no fingerprint`() {
        // SPEC §11: a failed migration fails closed. A record whose key cannot
        // be read is still dropped, however good its addresses look.
        val entry = entries()[0].apply { remove("fingerprint") }
        assertNull(TrustStore.TrustedPeer.fromJson(entry))
    }

    // -- recording a success ----------------------------------------------------

    @Test
    fun `a success moves its hint to the front with a fresh time`() {
        val fedora = record(Fixtures.IDENTITY_A_FINGERPRINT)
        val success = checkNotNull(
            AddressHint.from(
                InetSocketAddress.createUnresolved("FEDORA.local", 55432),
                1_900_000_000L,
            ),
        )
        val after = fedora.withSuccessfulAddress(success)
        assertEquals(
            listOf(
                AddressHint("fedora.local", 55432, 1_900_000_000L),
                AddressHint("192.168.68.72", 55432, 0),
                AddressHint("fe80::1%wlan0", 55432, 0),
            ),
            after.addressHints,
        )
        assertEquals(fedora.grantedCapabilities, after.grantedCapabilities)
        assertEquals(fedora.fingerprint.toHex(), after.fingerprint.toHex())
    }

    @Test
    fun `a success at a ninth address evicts the least recent one`() {
        var peer = record(DEBIAN_HEX)
        for (i in 1..9) {
            peer = peer.withSuccessfulAddress(AddressHint("10.0.0.$i", 55432, 1_800_000_000L + i))
        }
        assertEquals(8, peer.addressHints.size)
        assertEquals((9 downTo 2).map { "10.0.0.$it" }, peer.addressHints.map { it.host })
    }

    @Test
    fun `order is recency of success, not the wall clock`() {
        // A clock that went backwards must not evict the hint just proved.
        var peer = record(DEBIAN_HEX)
        for (i in 1..8) {
            peer = peer.withSuccessfulAddress(AddressHint("10.0.0.$i", 55432, 2_000_000_000L))
        }
        peer = peer.withSuccessfulAddress(AddressHint("10.0.0.99", 55432, 1L))
        assertEquals("10.0.0.99", peer.addressHints.first().host)
        assertEquals(8, peer.addressHints.size)
    }

    // -- revoked and hidden records keep no usable hints ------------------------

    @Test
    fun `revoked and hidden records read back with no hints, whatever the file kept`() {
        assertEquals(emptyList<AddressHint>(), record(REVOKED_HEX).addressHints)
        assertEquals(emptyList<AddressHint>(), record(TOMBSTONE_HEX).addressHints)

        // Even a hand-edited file that put structured hints on them.
        for (hex in listOf(REVOKED_HEX, TOMBSTONE_HEX)) {
            val entry = entries().single { it.getString("fingerprint") == hex }
                .put("addressHints", JSONArray().put(AddressHint("10.0.0.9", 55432).toJson()))
            val peer = checkNotNull(TrustStore.TrustedPeer.fromJson(entry))
            assertTrue(peer.revoked)
            assertEquals(emptyList<AddressHint>(), peer.addressHints)
            assertEquals(emptyList<InetSocketAddress>(), peer.dialAddresses())
        }
    }

    @Test
    fun `revoking and hiding remove usable hints`() {
        val fedora = record(Fixtures.IDENTITY_A_FINGERPRINT)
        assertTrue(fedora.dialAddresses().isNotEmpty())

        val revoked = fedora.asRevoked()
        assertEquals(emptyList<AddressHint>(), revoked.addressHints)
        assertEquals(emptyList<InetSocketAddress>(), revoked.dialAddresses())
        assertFalse(revoked.toJson().has("addressHints"))
        assertEquals(0, revoked.toJson().getJSONArray("addresses").length())

        val tombstone = revoked.asTombstone()
        assertEquals(emptyList<AddressHint>(), tombstone.addressHints)
        assertEquals(emptyList<InetSocketAddress>(), tombstone.dialAddresses())
    }

    @Test
    fun `a revoked record is not a destination even if a hint was left on it`() {
        // Belt and braces, as `allows` is for grants: the flag alone refuses.
        val stale = record(Fixtures.IDENTITY_A_FINGERPRINT).copy(revoked = true)
        assertTrue(stale.addressHints.isNotEmpty())
        assertEquals(emptyList<InetSocketAddress>(), stale.dialAddresses())
        assertEquals(stale, stale.withSuccessfulAddress(AddressHint("10.0.0.1", 55432, 1L)))
    }

    // -- normalization ----------------------------------------------------------

    @Test
    fun `hosts are normalized to one spelling per endpoint`() {
        assertEquals("fe80::1%wlan0", Endpoints.normalizeHost("[FE80:0000:0:0:0:0:0:1%wlan0]"))
        assertEquals("fe80::1%wlan0", Endpoints.normalizeHost("fe80:0:0:0:0:0:0:1%wlan0"))
        assertEquals("2001:db8::1:0:0:1", Endpoints.normalizeHost("2001:db8:0:0:1:0:0:1"))
        assertEquals("2001:db8:0:1:1:1:1:1", Endpoints.normalizeHost("2001:db8:0:1:1:1:1:1"))
        assertEquals("::", Endpoints.normalizeHost("0:0:0:0:0:0:0:0"))
        assertEquals("::ffff:192.168.68.72", Endpoints.normalizeHost("::FFFF:c0a8:4448"))
        assertEquals("::ffff:192.168.68.72", Endpoints.normalizeHost("::ffff:192.168.68.72"))
        assertEquals("192.168.68.72", Endpoints.normalizeHost(" 192.168.68.72 "))
        assertEquals("fedora.local", Endpoints.normalizeHost("Fedora.LOCAL."))
        assertEquals(
            AddressHint("fe80::1%wlan0", 55432),
            AddressHint.parseLegacy("[FE80:0:0::0001%wlan0]:55432"),
        )
    }

    @Test
    fun `what cannot name an endpoint is not a hint`() {
        val invalid = listOf(
            "", "[]", "1::2::3", "12345::1", "fe80::1%", "1:2:3:4:5:6:7:8:9",
            "010.1.1.1", "256.1.1.1", "a b", "a/b",
            // Digits and dots that are not a dotted quad are an address to
            // some resolvers, not a name.
            "1.2.3", "01.2.3.4.", "1.2.3.4.",
        )
        for (bad in invalid) {
            assertNull("'$bad' must not normalize", Endpoints.normalizeHost(bad))
        }
        assertNull(AddressHint.of("10.0.0.1", 0))
        assertNull(AddressHint.of("10.0.0.1", 65536))
        assertNotNull(AddressHint.of("10.0.0.1", 65535))
    }

    // -- only an authenticated session records a success ------------------------

    /**
     * A source assertion, as `PairingRecoveryTest` makes for pairing: the
     * property is *where* the call sits. The reconnect dialer records a hint
     * in the `Established` arm — after the pinned handshake and HELLO — and
     * nowhere else.
     */
    @Test
    fun `the dialer records a hint only for an established session`() {
        val source = File(
            "src/main/java/io/github/yurisismotto/pliwee/service/ConnectionService.kt",
        ).readText()
        val call = "trustStore.recordSuccessfulAddress("
        assertEquals("exactly one call", 1, Regex(Regex.escape(call)).findAll(source).count())

        val established = source.indexOf("is ConnectResult.Established ->")
        val nextArm = source.indexOf("is ConnectResult.PairingRequired ->", established)
        val at = source.indexOf(call)
        assertTrue("the Established arm was not found", established >= 0 && nextArm > established)
        assertTrue("the call must sit inside the Established arm", at in established..nextArm)

        // And nothing else in the app writes a hint behind the store's back.
        val main = File("src/main/java")
        val writers = main.walk()
            .filter { it.isFile && it.extension == "kt" }
            .filter { it.readText().contains(call) }
            .map { it.name }
            .toList()
        assertEquals(listOf("ConnectionService.kt"), writers)
    }

    private companion object {
        val DEBIAN_HEX = Fingerprint(ByteArray(32) { (2 + it).toByte() }).toHex()
        val REVOKED_HEX = Fingerprint(ByteArray(32) { (3 + it).toByte() }).toHex()
        val TOMBSTONE_HEX = Fingerprint(ByteArray(32) { (4 + it).toByte() }).toHex()
    }
}
