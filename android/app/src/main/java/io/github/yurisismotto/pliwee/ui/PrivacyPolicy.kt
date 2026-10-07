package io.github.yurisismotto.pliwee.ui

/**
 * Where Pliwee's privacy policy is published.
 *
 * Google Play requires the policy to be reachable both from the store listing
 * and from inside the app (Play v1 audit F4). The policy is written once, in
 * [SOURCE_PATH], beside the code it describes; [PAGE_PATH] is the static page
 * rendered from it, which GitHub Pages serves from `docs/` on `main` at [URL]
 * — no account, no script, no tracker. `PrivacyPolicyTest` pins this address
 * to that page and the page to its source, so moving or forking either breaks
 * the build instead of the link.
 */
object PrivacyPolicy {
    /** Repository-relative path of the policy's source of truth. */
    const val SOURCE_PATH = "docs/policy/PRIVACY-POLICY.md"

    /** Repository-relative path of the published page rendered from it. */
    const val PAGE_PATH = "docs/privacy/index.html"

    const val URL = "https://yurisismotto.github.io/pliwee/privacy/"
}
