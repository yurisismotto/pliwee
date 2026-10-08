package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.net.Endpoints
import java.net.InetSocketAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Endpoint selection.
 *
 * The defect these exist for: the phone took the first address discovery
 * handed back. On a dual-stack link that was an unscoped `fe80::` address,
 * which cannot be dialled at all, so the connection failed while a perfectly
 * good IPv4 address sat unused in the same record.
 */
class EndpointsTest {

    private fun at(host: String, port: Int = 47635) =
        InetSocketAddress.createUnresolved(host, port)

    // -- classification -----------------------------------------------------

    @Test
    fun `ordinary private IPv4 is dialable`() {
        assertEquals(Endpoints.Family.IPV4, Endpoints.classify("192.168.0.10"))
        assertEquals(Endpoints.Family.IPV4, Endpoints.classify("10.0.0.4"))
        assertEquals(Endpoints.Family.IPV4, Endpoints.classify("172.16.9.9"))
    }

    @Test
    fun `global and unique-local IPv6 are dialable`() {
        assertEquals(Endpoints.Family.IPV6_ROUTABLE, Endpoints.classify("2001:db8::1"))
        assertEquals(Endpoints.Family.IPV6_ROUTABLE, Endpoints.classify("fd00::5"))
        assertEquals(Endpoints.Family.IPV6_ROUTABLE, Endpoints.classify("[2001:db8::1]"))
    }

    @Test
    fun `link-local IPv6 is dialable only with a zone index`() {
        // The defect in one assertion: an address that names an interface it
        // does not identify is not an address we can connect to.
        assertNull(Endpoints.classify("fe80::1c2b:3aff:fe4d:5e6f"))
        assertEquals(
            Endpoints.Family.IPV6_LINK_LOCAL_SCOPED,
            Endpoints.classify("fe80::1c2b:3aff:fe4d:5e6f%wlan0"),
        )
        assertEquals(
            Endpoints.Family.IPV6_LINK_LOCAL_SCOPED,
            Endpoints.classify("fe80::1%12"),
        )
        // fe80::/10 covers more than the literal fe80 prefix.
        assertNull(Endpoints.classify("feb0::1"))
    }

    @Test
    fun `addresses that cannot be a peer are dropped`() {
        assertNull("loopback", Endpoints.classify("127.0.0.1"))
        assertNull("IPv6 loopback", Endpoints.classify("::1"))
        assertNull("unspecified", Endpoints.classify("0.0.0.0"))
        assertNull("IPv6 unspecified", Endpoints.classify("::"))
        assertNull("multicast", Endpoints.classify("224.0.0.251"))
        assertNull("IPv6 multicast", Endpoints.classify("ff02::fb"))
        assertNull("out of range octet", Endpoints.classify("192.168.0.999"))
        assertNull("empty", Endpoints.classify(""))
    }

    @Test
    fun `a hostname is kept but ranked last`() {
        assertEquals(Endpoints.Family.HOSTNAME, Endpoints.classify("fedora.local"))
    }

    // -- ordering -----------------------------------------------------------

    @Test
    fun `IPv4 is preferred over IPv6 on the networks this targets`() {
        val ordered = Endpoints.order(
            listOf(at("fe80::1%wlan0"), at("2001:db8::1"), at("192.168.0.10")),
        )
        assertEquals(
            listOf(at("192.168.0.10"), at("2001:db8::1"), at("fe80::1%wlan0")),
            ordered,
        )
    }

    @Test
    fun `an undialable address never consumes an attempt`() {
        val ordered = Endpoints.order(listOf(at("fe80::1"), at("192.168.0.10")))
        assertEquals(listOf(at("192.168.0.10")), ordered)
    }

    @Test
    fun `with no IPv6 the IPv4 address is still used`() {
        assertEquals(listOf(at("192.168.0.10")), Endpoints.order(listOf(at("192.168.0.10"))))
    }

    @Test
    fun `with no IPv4 a routable IPv6 address is still used`() {
        assertEquals(listOf(at("2001:db8::1")), Endpoints.order(listOf(at("2001:db8::1"))))
    }

    @Test
    fun `duplicates collapse and the caller's order survives among equals`() {
        val ordered = Endpoints.order(
            listOf(at("192.168.0.10"), at("192.168.0.11"), at("192.168.0.10")),
        )
        assertEquals(listOf(at("192.168.0.10"), at("192.168.0.11")), ordered)
    }

    @Test
    fun `the same host on a different port is not a duplicate`() {
        val ordered = Endpoints.order(listOf(at("192.168.0.10", 1), at("192.168.0.10", 2)))
        assertEquals(2, ordered.size)
    }

    @Test
    fun `link-local IPv4 sorts behind real addresses but is not discarded`() {
        val ordered = Endpoints.order(listOf(at("169.254.4.4"), at("192.168.0.10")))
        assertEquals(listOf(at("192.168.0.10"), at("169.254.4.4")), ordered)
    }

    @Test
    fun `an attempt round is capped`() {
        val many = (1..20).map { at("192.168.0.$it") }
        assertEquals(Endpoints.MAX_PER_ROUND, Endpoints.order(many).size)
        assertTrue(Endpoints.MAX_PER_ROUND in 2..8)
    }

    @Test
    fun `two spellings of one IPv6 address are one attempt`() {
        // A remembered hint is stored compressed; discovery resolves the same
        // address and hands back the long form. Two attempts at one place
        // would spend half a round on it.
        val ordered = Endpoints.order(
            listOf(at("fe80::1%wlan0"), at("fe80:0:0:0:0:0:0:1%wlan0"), at("2001:db8::1")),
        )
        assertEquals(listOf(at("2001:db8::1"), at("fe80::1%wlan0")), ordered)
    }

    // -- remembered hints and discovery ---------------------------------------

    @Test
    fun `stale remembered addresses cannot crowd out what discovery found`() {
        // The computer has had four DHCP leases, each proved once, and is now
        // at a fifth that only discovery knows. Every remembered address is
        // IPv4 like the new one, so the family sort cannot separate them.
        val remembered = listOf(
            at("192.168.68.75"),
            at("192.168.68.74"),
            at("192.168.68.73"),
            at("192.168.68.72"),
        )
        val discovered = listOf(at("192.168.68.76"))

        val round = Endpoints.order(Endpoints.withDiscovered(remembered, discovered))

        assertEquals(Endpoints.MAX_PER_ROUND, round.size)
        assertTrue("the discovered address is dialled this round", at("192.168.68.76") in round)
        assertEquals(
            "the most recent remembered address first, then discovery, then the rest",
            listOf(
                at("192.168.68.75"),
                at("192.168.68.76"),
                at("192.168.68.74"),
                at("192.168.68.73"),
            ),
            round,
        )
    }

    @Test
    fun `with one remembered address the order is what it always was`() {
        assertEquals(
            listOf(at("192.168.0.10"), at("192.168.0.11")),
            Endpoints.withDiscovered(listOf(at("192.168.0.10")), listOf(at("192.168.0.11"))),
        )
        assertEquals(
            listOf(at("192.168.0.11")),
            Endpoints.withDiscovered(emptyList(), listOf(at("192.168.0.11"))),
        )
    }

    @Test
    fun `a nonsense port is refused`() {
        assertTrue(Endpoints.order(listOf(at("192.168.0.10", 0))).isEmpty())
    }

    // -- round-tripping stored addresses ------------------------------------

    @Test
    fun `an IPv4 address round-trips through the trust store form`() {
        val address = at("192.168.0.10", 47635)
        assertEquals("192.168.0.10:47635", Endpoints.format(address))
        assertEquals(address, Endpoints.parse("192.168.0.10:47635"))
    }

    @Test
    fun `an IPv6 address round-trips with brackets`() {
        val address = at("2001:db8::1", 47635)
        assertEquals("[2001:db8::1]:47635", Endpoints.format(address))
        assertEquals(address, Endpoints.parse("[2001:db8::1]:47635"))
    }

    @Test
    fun `a scoped IPv6 address keeps its zone through a round-trip`() {
        val address = at("fe80::1%wlan0", 47635)
        val text = Endpoints.format(address)
        assertEquals("[fe80::1%wlan0]:47635", text)
        assertEquals(address, Endpoints.parse(text))
    }

    @Test
    fun `malformed stored addresses are ignored rather than crashing`() {
        assertNull(Endpoints.parse(""))
        assertNull(Endpoints.parse("192.168.0.10"))
        assertNull(Endpoints.parse("192.168.0.10:"))
        assertNull(Endpoints.parse(":47635"))
        assertNull(Endpoints.parse("192.168.0.10:notaport"))
        assertNull(Endpoints.parse("192.168.0.10:70000"))
    }
}
