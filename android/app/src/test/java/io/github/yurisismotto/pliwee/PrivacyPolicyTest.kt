package io.github.yurisismotto.pliwee

import io.github.yurisismotto.pliwee.ui.PrivacyPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.net.URI

/**
 * The in-app privacy link must point at the published Pliwee policy, and that
 * page must be the policy in the repository (Play v1 audit F4).
 *
 * Unit tests run with the `:app` module as the working directory, so the
 * repository root is two levels up.
 *
 * The link was `github.com/yurisismotto/OmniBridge/blob/main/…` until
 * versionCode 3: the OmniBridge repository is archived, and a Play submission
 * for `io.github.yurisismotto.pliwee` cannot declare a policy titled for
 * another product. The assertions on the old name are what stop that coming
 * back through a copy-paste.
 */
class PrivacyPolicyTest {

    private val repositoryRoot = File("../..").canonicalFile
    private val source = File(repositoryRoot, PrivacyPolicy.SOURCE_PATH)
    private val page = File(repositoryRoot, PrivacyPolicy.PAGE_PATH)

    @Test
    fun `the source and the published page exist in the repository`() {
        assertTrue("missing ${source.path}", source.isFile)
        assertTrue("the policy source is empty", source.length() > 0)
        assertTrue("missing ${page.path}", page.isFile)
        assertTrue("the policy page is empty", page.length() > 0)
    }

    @Test
    fun `the link is the GitHub Pages address of the Pliwee repository`() {
        assertEquals("https://yurisismotto.github.io/pliwee/privacy/", PrivacyPolicy.URL)
        val uri = URI(PrivacyPolicy.URL)
        assertEquals("https", uri.scheme)
        assertEquals("yurisismotto.github.io", uri.host)
        assertEquals("/pliwee/privacy/", uri.path)
        assertEquals(null, uri.query)
        assertEquals(null, uri.fragment)
    }

    @Test
    fun `the link serves the page file, given Pages publishes docs on main`() {
        // Pages maps https://<owner>.github.io/<repo>/<dir>/ to
        // docs/<dir>/index.html when the site is built from /docs.
        val servedDirectory = URI(PrivacyPolicy.URL).path.removePrefix("/pliwee/")
        assertEquals("docs/${servedDirectory}index.html", PrivacyPolicy.PAGE_PATH)
    }

    @Test
    fun `no address or path regresses to the OmniBridge name`() {
        for (value in listOf(PrivacyPolicy.URL, PrivacyPolicy.SOURCE_PATH, PrivacyPolicy.PAGE_PATH)) {
            assertFalse("old product name in $value", value.contains("omnibridge", ignoreCase = true))
        }
    }

    @Test
    fun `the policy is the Pliwee policy, in both forms`() {
        for (file in listOf(source, page)) {
            val text = file.readText()
            assertTrue("${file.name}: title", text.contains("Pliwee Privacy Policy"))
            assertTrue("${file.name}: package", text.contains("io.github.yurisismotto.pliwee"))
            assertTrue("${file.name}: repository", text.contains("https://github.com/yurisismotto/pliwee"))
            assertTrue("${file.name}: issues", text.contains("https://github.com/yurisismotto/pliwee/issues"))
            assertFalse(
                "${file.name} still names OmniBridge",
                text.contains("omnibridge", ignoreCase = true),
            )
        }
    }

    @Test
    fun `the published page says exactly what the source says`() {
        // Word for word, ignoring markup: a page that drifted from the source
        // would be a second policy, and the one Play reads is the page.
        val html = page.readText()
        val begin = html.indexOf(BEGIN_MARKER)
        val end = html.indexOf(END_MARKER)
        assertTrue("page markers missing", begin >= 0 && end > begin)
        val pageWords = words(htmlText(html.substring(begin + BEGIN_MARKER.length, end)))
        val sourceWords = words(markdownText(source.readText()))
        assertTrue("the source has no words", sourceWords.size > 500)
        assertEquals(
            "docs/privacy/index.html is out of date; run docs/policy/render-privacy-page.py",
            sourceWords,
            pageWords,
        )
    }

    @Test
    fun `the page loads nothing from anywhere`() {
        // No script, no stylesheet, font, image or frame: the page sets no
        // cookie and reaches no tracker because it asks for nothing.
        val html = page.readText().lowercase()
        for (forbidden in listOf("<script", "<link", "<img", "<iframe", "<object", "<embed", "src=", "@import", "url(")) {
            assertFalse("the page contains $forbidden", html.contains(forbidden))
        }
    }

    private fun words(text: String): List<String> =
        Regex("[\\p{L}\\p{N}]+").findAll(text).map { it.value }.toList()

    private fun markdownText(markdown: String): String = markdown
        .replace(Regex("(?m)^\\d+\\. "), "") // "1. " is <ol> numbering, not text
        .replace(Regex("\\]\\([^)]*\\)"), "]") // [text](target) keeps its text
        .replace(Regex("<(https?://[^>]+)>"), "$1") // <https://…> autolinks

    private fun htmlText(html: String): String = html
        .replace(Regex("<[^>]+>"), " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")

    private companion object {
        const val BEGIN_MARKER = "<!-- policy:begin -->"
        const val END_MARKER = "<!-- policy:end -->"
    }
}
