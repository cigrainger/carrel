#!/usr/bin/env python3
"""Check local Markdown targets and generated site links without network access."""

from html.parser import HTMLParser
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


class Page(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.ids = set()
        self.links = []
        self.feed(path.read_text())

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            self.ids.add(attrs["id"])
        for key in ("href", "src"):
            if key in attrs:
                self.links.append(attrs[key])


def main():
    errors = []
    markdown = list(ROOT.glob("*.md")) + list((ROOT / "docs").rglob("*.md"))
    markdown += list((ROOT / "site/src").rglob("*.md"))
    for file in markdown:
        text = re.sub(r"```.*?```", "", file.read_text(), flags=re.S)
        for raw in re.findall(r"!?\[[^\]]*\]\(([^)]+)\)", text):
            target = urlsplit(raw.split()[0].strip("<>"))
            if target.scheme or target.netloc or not target.path:
                continue
            path = (file.parent / unquote(target.path)).resolve()
            if not path.exists():
                errors.append(f"{file.relative_to(ROOT)}: missing {target.path}")

    book = ROOT / "site/book"
    pages = {file.resolve(): Page(file) for file in book.rglob("*.html")}
    if not pages:
        errors.append("build the site with mdbook before checking generated links")
    for file, page in pages.items():
        for raw in page.links:
            target = urlsplit(raw)
            if target.scheme or target.netloc:
                continue
            path = unquote(target.path)
            if path.startswith("/carrel/"):
                destination = book / path.removeprefix("/carrel/")
            elif path.startswith("/"):
                errors.append(f"{file.name}: URL escapes Pages project path: {raw}")
                continue
            else:
                destination = file.parent / path if path else file
            if destination.is_dir():
                destination /= "index.html"
            destination = destination.resolve()
            if not destination.exists():
                errors.append(f"{file.name}: missing {raw}")
            elif target.fragment and destination in pages:
                if unquote(target.fragment) not in pages[destination].ids:
                    errors.append(f"{file.name}: missing anchor {raw}")
    if errors:
        raise SystemExit("\n".join(sorted(set(errors))))
    print(f"Checked {len(markdown)} Markdown files and {len(pages)} rendered pages")


if __name__ == "__main__":
    main()
