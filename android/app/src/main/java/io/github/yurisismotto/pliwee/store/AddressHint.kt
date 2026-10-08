package io.github.yurisismotto.pliwee.store

import io.github.yurisismotto.pliwee.net.Endpoints
import java.net.InetSocketAddress
import org.json.JSONObject

/**
 * One place a trusted computer was last reached — a routing hint, never
 * identity.
 *
 * ## What it is for
 *
 * The dialer's fast path: an address that worked before is usually where the
 * computer still is, and trying it costs nothing, whereas browsing costs a
 * multicast lock and several seconds. That is the whole of its authority. The
 * SPKI pinned for the peer decides who answered on every dial, whichever hint
 * was dialled, so a hint that now leads to somebody else is a failed
 * handshake and nothing more (Mesh V2 SPEC §5, ADR-0022 §D3).
 *
 * ## What it holds
 *
 * A normalized host ([Endpoints.normalizeHost]), a TCP port, and
 * [lastSuccessUnix] — when an authenticated session last came up here, wall
 * clock, in seconds. That value is informational: it is never what orders or
 * evicts hints, because a wall clock can go backwards and a hint just proved
 * must not be evicted for it. Order is recency of success, most recent first,
 * and is the list's own order. `0` means "not known", which is what a hint
 * migrated from the old `addresses` strings carries.
 *
 * Anything that cannot name an endpoint — an empty host, a malformed
 * literal, an out-of-range port — is not a hint, and is dropped: hints are
 * non-authoritative and may be discarded safely (SPEC §11).
 */
data class AddressHint(
    val host: String,
    val port: Int,
    val lastSuccessUnix: Long = 0,
) {
    /** Whether [other] names the same endpoint. The time is not part of it. */
    fun sameEndpointAs(other: AddressHint): Boolean = host == other.host && port == other.port

    /** Unresolved, so building one never touches the network. */
    fun toSocketAddress(): InetSocketAddress = InetSocketAddress.createUnresolved(host, port)

    /** `host:port`, bracketed for IPv6 — what [Endpoints.parse] reads back. */
    fun format(): String = if (host.contains(':')) "[$host]:$port" else "$host:$port"

    fun toJson(): JSONObject = JSONObject().apply {
        put(KEY_HOST, host)
        put(KEY_PORT, port)
        put(KEY_LAST_SUCCESS, lastSuccessUnix)
    }

    companion object {
        /** At most this many hints per peer (Mesh V2 SPEC §5). */
        const val MAX_PER_PEER = 8

        private const val KEY_HOST = "host"
        private const val KEY_PORT = "port"
        private const val KEY_LAST_SUCCESS = "lastSuccessUnix"

        /** A hint, normalized, or null when the parts cannot name an endpoint. */
        fun of(host: String, port: Int, lastSuccessUnix: Long = 0): AddressHint? {
            if (port !in 1..65535) return null
            val normalized = Endpoints.normalizeHost(host) ?: return null
            return AddressHint(normalized, port, lastSuccessUnix.coerceAtLeast(0L))
        }

        /** The hint for an address an authenticated session just used. */
        fun from(address: InetSocketAddress, lastSuccessUnix: Long): AddressHint? {
            val host = address.hostString ?: return null
            return of(host, address.port, lastSuccessUnix)
        }

        /**
         * A stored `host:port` — the format of the old `addresses` array —
         * as a hint whose last success is unknown.
         */
        fun parseLegacy(text: String): AddressHint? =
            Endpoints.parse(text)?.let { of(it.hostString, it.port) }

        fun fromJson(entry: JSONObject): AddressHint? = of(
            host = entry.optString(KEY_HOST),
            port = entry.optInt(KEY_PORT, 0),
            lastSuccessUnix = entry.optLong(KEY_LAST_SUCCESS, 0),
        )

        /**
         * [hints] de-duplicated and capped at [MAX_PER_PEER].
         *
         * Deterministic and positional: the first occurrence of an endpoint
         * is the most recent one and is kept, later ones collapse into it,
         * and whatever is past the cap is dropped from the tail — the least
         * recently proved. Nothing here reads the clock.
         */
        fun bounded(hints: List<AddressHint>): List<AddressHint> {
            val kept = ArrayList<AddressHint>(MAX_PER_PEER)
            for (hint in hints) {
                if (kept.size == MAX_PER_PEER) break
                if (kept.none { it.sameEndpointAs(hint) }) kept += hint
            }
            return kept
        }

        /**
         * [hints] after an authenticated session came up at [success]: the
         * success moves to the front with its new time, the endpoint's older
         * entry collapses into it, and the list stays bounded.
         */
        fun recordSuccess(hints: List<AddressHint>, success: AddressHint): List<AddressHint> =
            bounded(listOf(success) + hints)
    }
}
