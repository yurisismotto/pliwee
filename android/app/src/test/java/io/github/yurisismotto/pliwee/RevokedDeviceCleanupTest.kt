package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.capability.BatteryCapability
import io.github.yurisismotto.pliwee.capability.ClipboardCapability
import io.github.yurisismotto.pliwee.capability.FilesCapability
import io.github.yurisismotto.pliwee.capability.NotificationsCapability
import io.github.yurisismotto.pliwee.capability.SensitiveCapabilities
import io.github.yurisismotto.pliwee.clipboard.ClipboardPolicy
import io.github.yurisismotto.pliwee.identity.Fingerprint
import io.github.yurisismotto.pliwee.notifications.NotificationPolicy
import io.github.yurisismotto.pliwee.store.AddressHint
import io.github.yurisismotto.pliwee.store.PeerTarget
import io.github.yurisismotto.pliwee.store.TrustStore
import io.github.yurisismotto.pliwee.ui.UiMapping
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Revoked computers, and taking them off the list.
 *
 * ## What was actually true before this sprint
 *
 * The brief for this work assumed the phone kept revoked computers on screen
 * forever. It did not: Android had **no revoked state at all**. The only thing
 * the person could do was "Forget this device", and that *deleted* the record.
 *
 * A deletion is not a revocation, and the difference is the whole point:
 *
 *  * the key became a computer this phone had never met, so a later pairing
 *    was an ordinary first pairing, with nothing anywhere recording that the
 *    person had once thrown it out;
 *  * the row simply vanished, giving no account of what happened to it;
 *  * and it happened on one tap, with no confirmation.
 *
 * So the lifecycle had to be built rather than extended, and these tests pin
 * it: `TRUSTED → REVOKED_VISIBLE → REVOKED_TOMBSTONE → (fresh pairing) →
 * TRUSTED`, with the tombstone continuing to mean "no" at every layer that
 * asks.
 *
 * ## Why these are JVM tests
 *
 * `TrustStore(context)` needs a `Context` and a `filesDir`, so the class
 * itself cannot be built here. Every *rule* it applies is a pure function on
 * its companion or on `TrustedPeer`, and that is deliberate — the rules are
 * what has to be right, and they are checked here rather than on a device.
 * `org.json` is on the unit-test classpath, so the persisted format is
 * round-tripped for real.
 */
class RevokedDeviceCleanupTest {

    // -- the world ----------------------------------------------------------

    private fun fingerprint(seed: Int) = Fingerprint(ByteArray(32) { (seed + it).toByte() })

    private val fedora = fingerprint(1)
    private val otherFedora = fingerprint(2)

    private fun peer(
        f: Fingerprint = fedora,
        name: String = "Fedora",
        grants: Set<String> = setOf(
            FilesCapability.ID,
            BatteryCapability.ID,
            ClipboardCapability.ID,
            NotificationsCapability.ID,
        ),
        addresses: List<String> = listOf("192.168.68.72:55432"),
        revoked: Boolean = false,
        hidden: Boolean = false,
    ) = TrustStore.TrustedPeer(
        deviceId = "795fec0868ebef8c3d7ad3e775dc6ed0",
        deviceName = name,
        fingerprint = f,
        pairedAtUnix = 1_700_000_000L,
        grantedCapabilities = grants,
        addressHints = addresses.mapNotNull(AddressHint::parseLegacy),
        revoked = revoked,
        hidden = hidden,
        clipboardPolicy = ClipboardPolicy(
            allowSend = true,
            allowReceive = true,
            autoReceive = true,
        ),
        notificationPolicy = NotificationPolicy(
            allowMirror = true,
            allowedApps = setOf("com.example.bank"),
        ),
    )

    private fun merge(existing: TrustStore.TrustedPeer?, paired: TrustStore.TrustedPeer) =
        TrustStore.TrustedPeer.mergePairing(existing, paired)

    /** What `PliweeApp.pair` builds from a session that has just come up. */
    private fun freshlyPaired(f: Fingerprint = fedora, name: String = "Fedora") = peer(
        f = f,
        name = name,
        grants = setOf(FilesCapability.ID, BatteryCapability.ID),
        addresses = listOf("192.168.68.99:55432"),
    ).copy(
        pairedAtUnix = 1_800_000_000L,
        clipboardPolicy = ClipboardPolicy(),
        notificationPolicy = NotificationPolicy(),
    )

    // -----------------------------------------------------------------------
    // A1 / A2 — what a row offers
    // -----------------------------------------------------------------------

    @Test
    fun `a revoked device offers remove from list and nothing else`() {
        val actions = UiMapping.deviceActions(peer().asRevoked())
        assertEquals(setOf(UiMapping.ACTION_REMOVE_FROM_LIST), actions)
    }

    @Test
    fun `a trusted device never offers remove from list`() {
        val actions = UiMapping.deviceActions(peer())
        assertFalse(
            "removing from the list must not be a back door to revoking",
            UiMapping.ACTION_REMOVE_FROM_LIST in actions,
        )
        assertTrue(UiMapping.ACTION_REVOKE in actions)
        assertTrue(UiMapping.ACTION_CONNECT in actions)
    }

    @Test
    fun `a revoked device is never offered connect`() {
        assertFalse(UiMapping.ACTION_CONNECT in UiMapping.deviceActions(peer().asRevoked()))
    }

    @Test
    fun `both destructive actions are declared destructive`() {
        assertTrue(UiMapping.ACTION_REVOKE in UiMapping.DESTRUCTIVE_ACTIONS)
        assertTrue(UiMapping.ACTION_REMOVE_FROM_LIST in UiMapping.DESTRUCTIVE_ACTIONS)
    }

    /** The state is a word, and it travels with the name. */
    @Test
    fun `a revoked device announces its state in words alongside its name`() {
        val description = UiMapping.deviceStateDescription(peer(name = "Fedora").asRevoked())
        assertTrue(description.contains("Fedora"))
        assertTrue(description.contains("revoked"))
        assertTrue(description.contains("can no longer connect"))
        // And a trusted one does not claim anything about revocation.
        assertEquals("Fedora", UiMapping.deviceStateDescription(peer(name = "Fedora")))
    }

    // -----------------------------------------------------------------------
    // Revoking: what goes, and what stays
    // -----------------------------------------------------------------------

    @Test
    fun `revoking keeps the record and clears everything the grant implied`() {
        val revoked = peer().asRevoked()

        assertTrue(revoked.revoked)
        assertFalse("still on the list until the person says otherwise", revoked.hidden)
        assertTrue(revoked.fingerprint.contentEquals(fedora))

        assertEquals(emptySet<String>(), revoked.grantedCapabilities)
        assertEquals(
            "a remembered address is a convenience for dialling something " +
                "this phone has just decided not to dial",
            emptyList<AddressHint>(),
            revoked.addressHints,
        )
        // Denied rather than merely default: a default policy serializes as
        // `allowMirror: true`, and its `knownApps` is every application on
        // this phone.
        assertEquals(ClipboardPolicy.DENIED, revoked.clipboardPolicy)
        assertEquals(NotificationPolicy.DENIED, revoked.notificationPolicy)
        assertTrue(revoked.notificationPolicy.allowedApps.isEmpty())
        assertTrue(revoked.notificationPolicy.knownApps.isEmpty())
        assertFalse(revoked.notificationPolicy.allowMirror)
    }

    @Test
    fun `a revoked record allows nothing even if a grant were left on it`() {
        // Belt and braces: `allows` refuses on the revoked flag alone, so a
        // record that somehow kept a grant still authorises nothing.
        val stale = peer().copy(revoked = true)
        assertFalse(stale.allows(FilesCapability.ID))
        assertFalse(stale.allows(ClipboardCapability.ID))
        assertFalse(stale.allows(NotificationsCapability.ID))
    }

    // -----------------------------------------------------------------------
    // The tombstone: what is kept, and what is purged
    // -----------------------------------------------------------------------

    @Test
    fun `a tombstone keeps the key and nothing else`() {
        val tombstone = peer().asRevoked().asTombstone()

        // Kept, because the rules read them.
        assertTrue(tombstone.fingerprint.contentEquals(fedora))
        assertTrue(tombstone.revoked)
        assertTrue(tombstone.hidden)
        assertFalse(tombstone.isListed())

        // Purged: none of it is needed to keep refusing a key.
        assertEquals("", tombstone.deviceId)
        assertEquals("", tombstone.deviceName)
        assertEquals(0L, tombstone.pairedAtUnix)
        assertEquals(emptySet<String>(), tombstone.grantedCapabilities)
        assertEquals(emptyList<AddressHint>(), tombstone.addressHints)
        assertEquals(ClipboardPolicy.DENIED, tombstone.clipboardPolicy)
        assertEquals(NotificationPolicy.DENIED, tombstone.notificationPolicy)
        assertTrue(tombstone.notificationPolicy.knownApps.isEmpty())
    }

    // -----------------------------------------------------------------------
    // A6 — a tombstoned computer is not a destination
    // -----------------------------------------------------------------------

    @Test
    fun `a revoked computer is not in the trusted set the coordinator resolves over`() {
        val records = listOf(peer().asRevoked())

        // The set every destination question is asked over.
        assertEquals(emptyList<TrustStore.TrustedPeer>(), TrustStore.trustedOf(records))
        assertEquals(
            "there is nothing to dial, and the loop says so rather than " +
                "backing off forever",
            PeerTarget.Resolution.NoTrustedPeer,
            PeerTarget.resolve(TrustStore.trustedOf(records), selectedHex = null),
        )
    }

    @Test
    fun `a stale choice naming a revoked computer resolves to nothing`() {
        val records = listOf(peer().asRevoked())
        val resolution = PeerTarget.resolve(
            TrustStore.trustedOf(records),
            selectedHex = fedora.toHex(),
        )
        assertNull(
            "a choice is not a route to trust",
            resolution.peerOrNull(),
        )
    }

    @Test
    fun `a revoked computer is listed until it is removed and never after`() {
        val revoked = peer().asRevoked()
        assertEquals(listOf(revoked), TrustStore.listedOf(listOf(revoked)))

        val tombstone = revoked.asTombstone()
        assertEquals(
            emptyList<TrustStore.TrustedPeer>(),
            TrustStore.listedOf(listOf(tombstone)),
        )
        assertEquals(
            "and it is still a record, which is the whole difference from a " +
                "deletion",
            1,
            listOf(tombstone).size,
        )
    }

    // -----------------------------------------------------------------------
    // A7 — the chosen computer
    // -----------------------------------------------------------------------

    @Test
    fun `removing the chosen computer clears the choice`() {
        assertNull(
            TrustStore.selectionAfterRemoval(
                selectedHex = fedora.toHex(),
                removedHex = fedora.toHex(),
            ),
        )
    }

    @Test
    fun `removing another computer leaves the choice alone`() {
        assertEquals(
            fedora.toHex(),
            TrustStore.selectionAfterRemoval(
                selectedHex = fedora.toHex(),
                removedHex = otherFedora.toHex(),
            ),
        )
    }

    @Test
    fun `removing never chooses a replacement`() {
        // Nothing can come out of this function but the old choice or null.
        // There is no path from a removal to a selection, which is what "does
        // not auto-select another peer" means at the layer that decides.
        assertNull(TrustStore.selectionAfterRemoval(null, fedora.toHex()))
        assertNull(
            TrustStore.selectionAfterRemoval(fedora.toHex(), fedora.toHex()),
        )
    }

    /**
     * And with the choice gone and two computers left, the app asks.
     *
     * The other half of A7: clearing must land in the state that *asks*, not
     * in one that quietly re-aims at whatever survived.
     */
    @Test
    fun `with the choice cleared and several computers left the app asks`() {
        val peers = listOf(peer(f = fedora), peer(f = otherFedora, name = "Debian"))
        val resolution = PeerTarget.resolve(peers, selectedHex = null)
        assertTrue(resolution is PeerTarget.Resolution.MustChoose)
        assertNull(resolution.peerOrNull())
    }

    // -----------------------------------------------------------------------
    // A8 / A14 — one key is one row, and a name is not an identity
    // -----------------------------------------------------------------------

    @Test
    fun `removing one computer leaves a same named computer untouched`() {
        // Same name, same device id, same address. Only the keys differ.
        val first = peer(f = fedora, name = "Fedora").asRevoked()
        val second = peer(f = otherFedora, name = "Fedora").asRevoked()
        val records = listOf(first, second)

        val after = TrustStore.upserted(records, first.asTombstone())

        assertEquals(2, after.size)
        val survivor = after.single { it.fingerprint.contentEquals(otherFedora) }
        assertFalse("the other Fedora was not touched", survivor.hidden)
        assertEquals("Fedora", survivor.deviceName)
        assertEquals(listOf(survivor), TrustStore.listedOf(after))
    }

    @Test
    fun `pairing over a tombstone yields one row, not two`() {
        val records = listOf(peer().asRevoked().asTombstone())
        val merged = merge(records.single(), freshlyPaired())
        val after = TrustStore.upserted(records, merged)

        assertEquals("one cryptographic identity is one row", 1, after.size)
        assertTrue(after.single().fingerprint.contentEquals(fedora))
        assertFalse(after.single().revoked)
        assertFalse(after.single().hidden)
    }

    // -----------------------------------------------------------------------
    // A9 / A11 / A13 — recovery through a real pairing, inheriting nothing
    // -----------------------------------------------------------------------

    @Test
    fun `pairing again after a revoke restores trust and no old grant`() {
        val tombstone = peer().asRevoked().asTombstone()
        val merged = merge(tombstone, freshlyPaired())

        assertFalse("the revocation is lifted by the pairing", merged.revoked)
        assertFalse("and the row comes back", merged.hidden)
        assertEquals("fresh display metadata", "Fedora", merged.deviceName)
        assertEquals(1_800_000_000L, merged.pairedAtUnix)

        // A11: not one sensitive capability, however the negotiation went.
        for (sensitive in SensitiveCapabilities.NEVER_AUTO_GRANTED) {
            assertFalse(
                "$sensitive must never be granted by a pairing",
                merged.allows(sensitive),
            )
        }

        // A13: the old policies are not resurrected either. The bank app the
        // person had named for the *previous* relationship is gone.
        assertEquals(ClipboardPolicy(), merged.clipboardPolicy)
        assertEquals(NotificationPolicy(), merged.notificationPolicy)
        assertTrue(merged.notificationPolicy.allowedApps.isEmpty())

        // And the stale address is not remembered back into the record.
        assertEquals(listOf("192.168.68.99:55432"), merged.addressHints.map { it.format() })
    }

    /**
     * The same, one step earlier in the lifecycle: a *visible* revoked record.
     *
     * Removing it from the list must not be what protects the grants — the
     * revocation already does. Otherwise a person who revoked but did not
     * tidy up would get their old grants back on the next pairing, which is
     * the more likely path of the two.
     */
    @Test
    fun `pairing again over a visible revoked record inherits nothing either`() {
        val merged = merge(peer().asRevoked(), freshlyPaired())
        assertFalse(merged.revoked)
        assertEquals(
            setOf(FilesCapability.ID, BatteryCapability.ID),
            merged.grantedCapabilities,
        )
        assertEquals(NotificationPolicy(), merged.notificationPolicy)
    }

    /**
     * The regression guard for UX-DEBT-02, which the rule above could have
     * broken.
     *
     * A *desktop-side* revoke leaves this phone's own record untouched — the
     * phone never decided anything — so re-pairing after one must still keep
     * the grants and the app list the person chose. Only the phone owner's own
     * revoke erases them.
     */
    @Test
    fun `a desktop side revoke still leaves this phone's decisions intact`() {
        val mine = peer()
        val merged = merge(mine, freshlyPaired())

        assertTrue("the person's own clipboard grant survives", merged.allows(ClipboardCapability.ID))
        assertTrue(merged.allows(NotificationsCapability.ID))
        assertEquals(setOf("com.example.bank"), merged.notificationPolicy.allowedApps)
        assertTrue(merged.clipboardPolicy.allowReceive)
    }

    // -----------------------------------------------------------------------
    // A10 — a failed pairing changes nothing
    // -----------------------------------------------------------------------

    @Test
    fun `a failed pairing leaves the tombstone exactly as it was`() {
        // `mergePairing` is reached only after a proof succeeded. A failure
        // never calls it, so the record is whatever it was — this pins that
        // the record *is* still there to be unchanged, which is the half a
        // deletion used to get wrong.
        val tombstone = peer().asRevoked().asTombstone()
        val records = listOf(tombstone)

        assertEquals(1, records.size)
        assertTrue(records.single().revoked)
        assertTrue(records.single().hidden)
        assertEquals(emptyList<TrustStore.TrustedPeer>(), TrustStore.trustedOf(records))
        assertEquals(emptyList<TrustStore.TrustedPeer>(), TrustStore.listedOf(records))
    }

    // -----------------------------------------------------------------------
    // A12 — a different key is a different computer
    // -----------------------------------------------------------------------

    @Test
    fun `a tombstone gives nothing to a different fingerprint`() {
        val tombstone = peer(f = fedora).asRevoked().asTombstone()
        // Key B, presenting the same name. The caller looks a record up by
        // fingerprint, so B finds nothing; the check inside `mergePairing` is
        // belt and braces on top of that.
        val merged = merge(tombstone, freshlyPaired(f = otherFedora, name = "Fedora"))

        assertTrue(merged.fingerprint.contentEquals(otherFedora))
        assertFalse("B is trusted on its own terms", merged.revoked)
        assertFalse("and visible, not inheriting A's tombstone", merged.hidden)
        assertEquals(
            setOf(FilesCapability.ID, BatteryCapability.ID),
            merged.grantedCapabilities,
        )

        // And A is untouched: two keys, two records.
        val after = TrustStore.upserted(listOf(tombstone), merged)
        assertEquals(2, after.size)
        val a = after.single { it.fingerprint.contentEquals(fedora) }
        assertTrue(a.revoked && a.hidden)
        assertEquals(listOf(merged), TrustStore.listedOf(after))
    }

    // -----------------------------------------------------------------------
    // A5 — the persisted format, round-tripped for real
    // -----------------------------------------------------------------------

    @Test
    fun `a tombstone survives being written and read back`() {
        val tombstone = peer().asRevoked().asTombstone()
        val restored = TrustStore.TrustedPeer.fromJson(tombstone.toJson())

        assertNotNull(restored)
        checkNotNull(restored)
        assertTrue(restored.fingerprint.contentEquals(fedora))
        assertTrue("a tombstone that did not survive a reload is a deletion", restored.revoked)
        assertTrue(restored.hidden)
        assertEquals("", restored.deviceName)
        assertEquals(emptySet<String>(), restored.grantedCapabilities)
        assertEquals(emptyList<AddressHint>(), restored.addressHints)
    }

    @Test
    fun `a revoked but visible record survives being written and read back`() {
        val restored = TrustStore.TrustedPeer.fromJson(peer().asRevoked().toJson())
        checkNotNull(restored)
        assertTrue(restored.revoked)
        assertFalse("and it is still on the list", restored.hidden)
    }

    // -----------------------------------------------------------------------
    // Migration, and failing closed
    // -----------------------------------------------------------------------

    @Test
    fun `a record written before this sprint reads as trusted and visible`() {
        // Exactly what an older build wrote: neither key present.
        val old = peer().toJson()
        old.remove("revoked")
        old.remove("hiddenFromUi")

        val restored = checkNotNull(TrustStore.TrustedPeer.fromJson(old))
        assertFalse("nobody's computer is revoked by an upgrade", restored.revoked)
        assertFalse("and nobody's row disappears", restored.hidden)
        assertTrue("and the grants they made are still theirs", restored.allows(FilesCapability.ID))
        assertTrue(restored.allows(ClipboardCapability.ID))
        assertEquals(setOf("com.example.bank"), restored.notificationPolicy.allowedApps)
    }

    @Test
    fun `an ordinary trusted record is written exactly as an older build wrote it`() {
        // The flags are written only when true, so a file full of trusted
        // computers is byte-identical to what a build without this feature
        // produced — which is what makes a downgrade uneventful.
        val json = peer().toJson()
        assertFalse(json.has("revoked"))
        assertFalse(json.has("hiddenFromUi"))
    }

    @Test
    fun `a record that says hidden but not revoked is read as revoked`() {
        // The one dangerous combination: out of sight and still trusted. A
        // file that says this is hand-edited or damaged, and believing it
        // would mean a computer nobody can see keeping its grants.
        val tampered = peer().toJson().apply {
            put("hiddenFromUi", true)
            put("revoked", false)
        }
        val restored = checkNotNull(TrustStore.TrustedPeer.fromJson(tampered))
        assertTrue("hidden implies revoked, decided here not believed", restored.revoked)
        assertTrue(restored.hidden)
        assertFalse(restored.allows(FilesCapability.ID))
        assertEquals(emptyList<TrustStore.TrustedPeer>(), TrustStore.trustedOf(listOf(restored)))
    }

    @Test
    fun `a record with no usable fingerprint is dropped rather than guessed`() {
        val nameless = JSONObject().apply { put("deviceName", "Fedora") }
        assertNull(TrustStore.TrustedPeer.fromJson(nameless))
        val garbled = peer().toJson().apply { put("fingerprint", "not-hex") }
        assertNull(TrustStore.TrustedPeer.fromJson(garbled))
    }

    @Test
    fun `nothing in the persisted record could hold content`() {
        // The privacy claim, checked against the bytes rather than asserted.
        // A tombstone is a fingerprint, two flags and the empty defaults.
        val json = peer().asRevoked().asTombstone().toJson()
        val keys = generateSequence(json.keys()) { json.keys() }.first().asSequence().toSet()
        assertEquals(
            setOf(
                "deviceId",
                "deviceName",
                "fingerprint",
                "pairedAtUnix",
                "grantedCapabilities",
                "addresses",
                "revoked",
                "hiddenFromUi",
                "clipboardPolicy",
                "notificationPolicy",
            ),
            keys,
        )
        val text = json.toString()
        assertFalse(text.contains("192.168"))
        assertFalse(text.contains("com.example.bank"))
        assertFalse(text.contains("Fedora"))
    }
}
