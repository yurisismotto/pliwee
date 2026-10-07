#!/usr/bin/env python3
"""Render docs/policy/PRIVACY-POLICY.md into docs/privacy/index.html.

The Markdown is the policy's single source of truth. The HTML is what GitHub
Pages serves at https://yurisismotto.github.io/pliwee/privacy/, the address the
Android app and the Play listing declare. Never edit the HTML by hand: edit the
Markdown and run this script.

    docs/policy/render-privacy-page.py           # rewrite the page
    docs/policy/render-privacy-page.py --check   # exit 1 if it is stale

`PrivacyPolicyTest` independently checks that the page says, word for word,
what the source says, and that it loads nothing from anywhere.

Standard library only, and only the Markdown the policy uses: headings,
paragraphs, `*` and `1.` lists, pipe tables, **bold**, `code`, [links](url)
and <https://autolinks>. Anything else is refused rather than guessed at.
"""

import html
import re
import sys
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent
SOURCE = DOCS / "policy" / "PRIVACY-POLICY.md"
PAGE = DOCS / "privacy" / "index.html"
REPOSITORY = "https://github.com/yurisismotto/pliwee"


def inline(text):
    out = html.escape(text, quote=True)
    out = re.sub(r"`([^`]+)`", r"<code>\1</code>", out)
    out = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", out)
    out = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", r'<a href="\2">\1</a>', out)
    out = re.sub(r"&lt;(https://[^&\s]+)&gt;", r'<a href="\1">\1</a>', out)
    if "*" in re.sub(r"<[^>]+>", "", out):
        raise SystemExit(f"unsupported emphasis in: {text!r}")
    return out


def table(rows):
    cells = [[c.strip() for c in r.strip().strip("|").split("|")] for r in rows]
    if len(cells) < 2 or not all(re.fullmatch(r":?-+:?", c) for c in cells[1]):
        raise SystemExit(f"malformed table: {rows[0]!r}")
    head = "".join(f"<th>{inline(c)}</th>" for c in cells[0])
    body = "".join(
        "<tr>" + "".join(f"<td>{inline(c)}</td>" for c in row) + "</tr>\n"
        for row in cells[2:]
    )
    return f'<div class="table"><table>\n<thead><tr>{head}</tr></thead>\n<tbody>\n{body}</tbody>\n</table></div>'


def render(markdown):
    blocks, title = [], None
    lines = markdown.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
            continue
        heading = re.match(r"(#{1,3}) (.+)", line)
        if heading:
            level, text = len(heading.group(1)), heading.group(2)
            if level == 1:
                title = text
            slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
            blocks.append(f'<h{level} id="{slug}">{inline(text)}</h{level}>')
            i += 1
        elif line.startswith("|"):
            rows = []
            while i < len(lines) and lines[i].startswith("|"):
                rows.append(lines[i])
                i += 1
            blocks.append(table(rows))
        elif re.match(r"(\* |\d+\. )", line):
            ordered = not line.startswith("* ")
            marker = r"\d+\. " if ordered else r"\* "
            items = []
            while i < len(lines) and lines[i].strip():
                if re.match(marker, lines[i]):
                    items.append(re.sub("^" + marker, "", lines[i]))
                elif lines[i].startswith("  "):
                    items[-1] += " " + lines[i].strip()
                else:
                    raise SystemExit(f"unsupported list line: {lines[i]!r}")
                i += 1
            tag = "ol" if ordered else "ul"
            body = "\n".join(f"<li>{inline(item)}</li>" for item in items)
            blocks.append(f"<{tag}>\n{body}\n</{tag}>")
        elif re.match(r"(>|```|    |-{3,}$|#{4,})", line):
            raise SystemExit(f"unsupported Markdown: {line!r}")
        else:
            para = []
            while i < len(lines) and lines[i].strip() and not re.match(r"(#|\||\* |\d+\. )", lines[i]):
                para.append(lines[i].strip())
                i += 1
            blocks.append(f"<p>{inline(' '.join(para))}</p>")
    if title != "Pliwee Privacy Policy":
        raise SystemExit(f"unexpected title: {title!r}")
    return "\n\n".join(blocks)


TEMPLATE = """<!doctype html>
<!--
  GENERATED from docs/policy/PRIVACY-POLICY.md by
  docs/policy/render-privacy-page.py. Do not edit by hand.
-->
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<meta name="description" content="What the Pliwee Android app and desktop software access, where it goes, and how to stop it.">
<title>Pliwee Privacy Policy</title>
<style>
:root {
  --bg: #f7f9fa; --surface: #ffffff; --text: #17242b; --muted: #52616a;
  --line: #dbe3e7; --accent: #0f7f7a; --code: #eef3f5;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0e1518; --surface: #152025; --text: #e4ecef; --muted: #9db0b8;
    --line: #26353c; --accent: #45c4bc; --code: #1d2b31;
  }
}
* { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; }
body {
  margin: 0; background: var(--bg); color: var(--text);
  font: 16px/1.6 system-ui, -apple-system, "Segoe UI", Roboto, "Noto Sans", sans-serif;
}
header, main, footer { max-width: 46rem; margin: 0 auto; padding: 0 16px; }
header { padding-top: 2rem; }
.brand { font-weight: 700; letter-spacing: .02em; color: var(--accent); text-decoration: none; }
main {
  background: var(--surface); border: 1px solid var(--line); border-radius: 12px;
  padding: 1.5rem 1.25rem; margin-top: 1rem;
}
h1 { font-size: 1.9rem; line-height: 1.2; margin: 0 0 1rem; }
h2 { font-size: 1.35rem; margin: 2.2rem 0 .6rem; padding-top: 1rem; border-top: 1px solid var(--line); }
h3 { font-size: 1.1rem; margin: 1.6rem 0 .4rem; }
p, ul, ol { margin: .6rem 0; }
li { margin: .3rem 0; }
a { color: var(--accent); overflow-wrap: anywhere; }
code { background: var(--code); border-radius: 4px; padding: .05em .3em; font-size: .92em; overflow-wrap: anywhere; }
.table { overflow-x: auto; margin: .8rem 0; }
table { border-collapse: collapse; width: 100%; font-size: .95rem; }
th, td { text-align: left; vertical-align: top; padding: .5rem .6rem; border: 1px solid var(--line); }
th { background: var(--code); }
footer { color: var(--muted); font-size: .9rem; padding-top: 1.25rem; padding-bottom: 2.5rem; }
footer p { margin: .3rem 0; }
@media (min-width: 640px) { main { padding: 2rem 2.5rem; } }
</style>
</head>
<body>
<header><a class="brand" href="{repository}">Pliwee</a></header>
<main>
<!-- policy:begin -->
{policy}
<!-- policy:end -->
</main>
<footer>
<p>Source: <a href="{repository}/blob/main/docs/policy/PRIVACY-POLICY.md">docs/policy/PRIVACY-POLICY.md</a> · history: <a href="{repository}/commits/main/docs/policy/PRIVACY-POLICY.md">changes to this policy</a></p>
<p>Questions: <a href="{repository}/issues">{repository}/issues</a></p>
<p>This page uses no cookies, scripts, trackers or external resources.</p>
</footer>
</body>
</html>
"""


def main(argv):
    page = TEMPLATE.replace("{repository}", REPOSITORY).replace(
        "{policy}", render(SOURCE.read_text(encoding="utf-8"))
    )
    if argv[1:] == ["--check"]:
        if not PAGE.is_file() or PAGE.read_text(encoding="utf-8") != page:
            print(f"{PAGE} is out of date: run {Path(__file__).name}", file=sys.stderr)
            return 1
        print(f"{PAGE.relative_to(DOCS.parent)} is up to date")
        return 0
    if argv[1:]:
        print(__doc__, file=sys.stderr)
        return 2
    PAGE.parent.mkdir(exist_ok=True)
    PAGE.write_text(page, encoding="utf-8")
    print(f"wrote {PAGE.relative_to(DOCS.parent)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
