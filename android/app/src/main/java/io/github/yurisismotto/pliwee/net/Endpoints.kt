package io.github.yurisismotto.pliwee.net

import java.net.InetSocketAddress

/**
 * Turns "everywhere this computer might be" into "what to dial, in order".
 *
 * ## Why this exists
 *
 * Discovery does not hand back one address, it hands back a set: a daemon on
 * a dual-stack link publishes an `A` record and one or more `AAAA` records,
 * and the phone may also remember where the computer was last time. Picking
 * the first of those arbitrarily is a coin flip, and it lost: the link-local
 * `fe80::` address sorted first and nothing could connect.
 *
 * ## Ordering, and why it is not RFC 8305
 *
 * Happy Eyeballs races an IPv6 and an IPv4 socket and keeps whichever answers
 * first. We deliberately do not: racing sockets against a device that must not
 * waste radio is the wrong trade, and the cost we are avoiding — one failed
 * connect — is usually an immediate `ECONNREFUSED` or `EHOSTUNREACH`, not a
 * timeout. So attempts are **sequential**, at most [MAX_PER_ROUND] of them,
 * and the order matters more than it would in a race:
 *
 *  1. **IPv4.** On the consumer mesh Wi-Fi this targets, IPv4 is what reliably
 *     carries traffic between access points; IPv6 on such networks is often
 *     link-local only.
 *  2. **Routable IPv6** — global or unique-local. Real dual-stack, used when
 *     it is there.
 *  3. **Link-local IPv6 that carries a zone**, e.g. `fe80::1%wlan0`. Usable,
 *     but only on the one interface it names.
 *  4. **IPv4 link-local** (`169.254/16`). Only meaningful with no DHCP.
 *  5. **Hostnames.** Last: resolving one costs a lookup that may not answer.
 *
 * Anything that cannot be dialled at all is dropped rather than sorted, so a
 * connection attempt is never spent on an address that could not have worked:
 * loopback, the unspecified address, multicast, an out-of-range port, and —
 * the case that caused the defect — a link-local IPv6 address with no zone
 * index, which names an interface it does not identify.
 *
 * Everything here is a pure function of strings. Nothing resolves a name and
 * nothing touches the network, so it is all directly testable.
 */
object Endpoints {

    /**
     * How many addresses one connection round will try before backing off.
     *
     * A cap is needed: a hostile or broken responder can publish an unbounded
     * number of records, and "try them all" would be a battery attack.
     */
    const val MAX_PER_ROUND = 4

    /** Dialability class, in preference order. */
    enum class Family {
        IPV4,
        IPV6_ROUTABLE,
        IPV6_LINK_LOCAL_SCOPED,
        IPV4_LINK_LOCAL,
        HOSTNAME,
    }

    /**
     * Which class a host literal falls into, or `null` if it cannot be
     * dialled at all.
     */
    fun classify(host: String): Family? {
        val bare = host.removeSurrounding("[", "]").trim()
        if (bare.isEmpty()) return null

        return if (bare.contains(':')) classifyIpv6(bare) else classifyIpv4OrName(bare)
    }

    private fun classifyIpv6(literal: String): Family? {
        val scopeAt = literal.indexOf('%')
        val address = if (scopeAt >= 0) literal.substring(0, scopeAt) else literal
        val scope = if (scopeAt >= 0) literal.substring(scopeAt + 1) else ""

        val compact = address.lowercase()
        if (compact == "::1" || compact == "::") return null

        val firstHextet = compact.substringBefore(':')
            .takeIf { it.isNotEmpty() }
            ?.toIntOrNull(16)
            ?: return if (compact.startsWith("::")) Family.IPV6_ROUTABLE else null

        // ff00::/8 is multicast. A unicast connection to it is meaningless.
        if (firstHextet and 0xff00 == 0xff00) return null

        // fe80::/10 is link-local. Without a zone index the address does not
        // say which interface it lives on, and connect() cannot guess.
        if (firstHextet and 0xffc0 == 0xfe80) {
            return if (scope.isNotEmpty()) Family.IPV6_LINK_LOCAL_SCOPED else null
        }

        return Family.IPV6_ROUTABLE
    }

    private fun classifyIpv4OrName(literal: String): Family? {
        val octets = literal.split('.')
        if (octets.size != 4 || octets.any { it.isEmpty() || it.toIntOrNull() == null }) {
            // Not a dotted quad: treat it as a name and let the resolver
            // decide. A malformed name simply fails to connect.
            return Family.HOSTNAME.takeIf { literal.none { c -> c == '/' || c == ' ' } }
        }

        val values = octets.map { it.toInt() }
        if (values.any { it !in 0..255 }) return null

        return when {
            values[0] == 127 -> null
            values == listOf(0, 0, 0, 0) -> null
            values[0] in 224..239 -> null
            values[0] == 169 && values[1] == 254 -> Family.IPV4_LINK_LOCAL
            else -> Family.IPV4
        }
    }

    /**
     * Orders candidates best-first, dropping the undialable and capping the
     * result at [MAX_PER_ROUND].
     *
     * The sort is stable, so among equally-preferred addresses the caller's
     * own order survives — which is what keeps the address that worked last
     * time ahead of one that was merely discovered.
     */
    fun order(
        candidates: List<InetSocketAddress>,
        limit: Int = MAX_PER_ROUND,
    ): List<InetSocketAddress> = candidates
        .asSequence()
        .filter { it.port in 1..65535 }
        // Compared normalized, so a remembered `fe80::1%wlan0` and the same
        // address resolved by discovery as `fe80:0:0:0:0:0:0:1%wlan0` are one
        // attempt rather than two of the round's few.
        .distinctBy { "${normalizeHost(it.hostString) ?: it.hostString}:${it.port}" }
        .mapNotNull { address -> classify(address.hostString)?.let { it to address } }
        .sortedBy { it.first.ordinal }
        .map { it.second }
        .take(limit)
        .toList()

    /** Parses a stored `host:port`, or `null` if it is not one. */
    fun parse(text: String): InetSocketAddress? {
        val trimmed = text.trim()
        // An IPv6 literal is full of colons, so the port is after the last
        // one — and after the closing bracket when the form is `[::1]:80`.
        val separator = trimmed.lastIndexOf(':')
        if (separator <= 0 || separator == trimmed.length - 1) return null
        if (trimmed.contains(']') && separator < trimmed.lastIndexOf(']')) return null

        val port = trimmed.substring(separator + 1).toIntOrNull() ?: return null
        if (port !in 1..65535) return null

        val host = trimmed.substring(0, separator).removeSurrounding("[", "]")
        if (host.isEmpty()) return null

        return runCatching { InetSocketAddress.createUnresolved(host, port) }.getOrNull()
    }

    /** Renders an address the way [parse] expects to read it back. */
    fun format(address: InetSocketAddress): String {
        val host = address.hostString
        return if (host.contains(':')) "[$host]:${address.port}" else "$host:${address.port}"
    }

    /**
     * The candidates for a round that has run discovery: the most recently
     * proved address, then what discovery found, then the older remembered
     * ones.
     *
     * Remembered addresses are kept per peer, several of them, and the
     * round is capped at [MAX_PER_ROUND]. Putting all of them first would let
     * addresses the computer has since left — an old DHCP lease, another
     * network — fill the round in front of the one discovery just saw, every
     * round, so the computer could never be reached again without re-pairing.
     * The most recent one keeps its place ahead of discovery, as the single
     * remembered address always had; the rest follow it.
     *
     * Unordered: [order] still decides the family preference, and its sort is
     * stable, so this order survives within a family.
     */
    fun withDiscovered(
        remembered: List<InetSocketAddress>,
        discovered: List<InetSocketAddress>,
    ): List<InetSocketAddress> = remembered.take(1) + discovered + remembered.drop(1)

    /**
     * One spelling per host, or null when [host] cannot be one.
     *
     * IPv6 literals in RFC 5952 form — lowercase, zeros compressed, an
     * IPv4-mapped address with its dotted tail — with any zone kept as
     * written; IPv4 in plain dotted decimal; names in lowercase without a
     * trailing dot. Pure string work: no resolver is asked, so normalizing a
     * name costs nothing and leaks nothing.
     *
     * Digits and dots that are not exactly a dotted quad are refused rather
     * than read as a name: `1.2.3` and `010.0.0.1` are an address to some
     * resolvers and a different one to others, and an endpoint must not mean
     * two places.
     */
    fun normalizeHost(host: String): String? {
        val bare = host.trim().removeSurrounding("[", "]")
        if (bare.isEmpty() || bare.length > MAX_HOST_LENGTH) return null
        if (bare.any { it.isWhitespace() || it.isISOControl() || it == '/' }) return null
        return when {
            bare.contains(':') -> normalizeIpv6(bare)
            bare.all { it in '0'..'9' || it == '.' } -> ipv4Octets(bare)?.joinToString(".")
            else -> bare.lowercase().removeSuffix(".").takeIf { it.isNotEmpty() }
        }
    }

    /** A host name longer than DNS allows is not one. */
    private const val MAX_HOST_LENGTH = 253

    /** A strict dotted quad: four decimal octets, no leading zeros. */
    private fun ipv4Octets(text: String): List<Int>? {
        val parts = text.split('.')
        if (parts.size != 4) return null
        return parts.map { part ->
            if (part.isEmpty() || part.length > 3 || part.any { it !in '0'..'9' }) return null
            if (part.length > 1 && part[0] == '0') return null
            part.toInt().takeIf { it in 0..255 } ?: return null
        }
    }

    private fun normalizeIpv6(text: String): String? {
        val scopeAt = text.indexOf('%')
        val literal = if (scopeAt >= 0) text.substring(0, scopeAt) else text
        val zone = if (scopeAt >= 0) text.substring(scopeAt + 1) else null
        // A `%` with nothing after it names no interface.
        if (zone != null && (zone.isEmpty() || zone.contains('%'))) return null
        val groups = ipv6Groups(literal) ?: return null
        val rendered = rfc5952(groups)
        return if (zone != null) "$rendered%$zone" else rendered
    }

    /** The eight 16-bit groups of an IPv6 literal, or null. */
    private fun ipv6Groups(text: String): IntArray? {
        val gap = text.indexOf("::")
        if (gap >= 0 && text.indexOf("::", gap + 1) >= 0) return null
        val head = if (gap >= 0) text.substring(0, gap) else text
        val tail = if (gap >= 0) text.substring(gap + 2) else ""
        // An embedded IPv4 tail is only allowed in the last position.
        val headGroups = hextets(head, allowIpv4Tail = gap < 0) ?: return null
        val tailGroups = hextets(tail, allowIpv4Tail = true) ?: return null
        val count = headGroups.size + tailGroups.size
        if (if (gap >= 0) count > 7 else count != 8) return null
        val out = IntArray(8)
        headGroups.forEachIndexed { i, v -> out[i] = v }
        tailGroups.forEachIndexed { i, v -> out[8 - tailGroups.size + i] = v }
        return out
    }

    private fun hextets(part: String, allowIpv4Tail: Boolean): List<Int>? {
        if (part.isEmpty()) return emptyList()
        val pieces = part.split(':')
        val out = ArrayList<Int>(8)
        for ((index, piece) in pieces.withIndex()) {
            if (allowIpv4Tail && index == pieces.lastIndex && piece.contains('.')) {
                val v4 = ipv4Octets(piece) ?: return null
                out += (v4[0] shl 8) or v4[1]
                out += (v4[2] shl 8) or v4[3]
            } else {
                if (piece.length !in 1..4) return null
                if (piece.any { Character.digit(it, 16) < 0 }) return null
                out += piece.toInt(16)
            }
        }
        return out
    }

    /** RFC 5952 §4 and §5. */
    private fun rfc5952(groups: IntArray): String {
        // §5: an IPv4-mapped address keeps its dotted tail.
        if ((0 until 5).all { groups[it] == 0 } && groups[5] == 0xffff) {
            val v4 = listOf(groups[6] shr 8, groups[6] and 0xff, groups[7] shr 8, groups[7] and 0xff)
            return "::ffff:" + v4.joinToString(".")
        }
        // §4.2.3: the longest run of two or more zero groups, the first on a tie.
        var bestStart = -1
        var bestLength = 1
        var i = 0
        while (i < groups.size) {
            if (groups[i] != 0) {
                i++
                continue
            }
            val start = i
            while (i < groups.size && groups[i] == 0) i++
            if (i - start > bestLength) {
                bestStart = start
                bestLength = i - start
            }
        }
        val hex = { range: IntRange -> range.joinToString(":") { Integer.toHexString(groups[it]) } }
        if (bestStart < 0) return hex(groups.indices)
        return hex(0 until bestStart) + "::" + hex(bestStart + bestLength until groups.size)
    }
}
