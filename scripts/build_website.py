#!/usr/bin/env python3
"""Build the small static Pages site from shared layout and authored page bodies.

No framework/runtime/CDN dependencies. Versions and service distances are read
from the repository instead of maintaining competing copies in HTML.
"""
import argparse
import html
import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "website"


def render():
    config = json.loads((SITE / "site.json").read_text())
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
    reference = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    values = {"bevy": workspace["workspace"]["dependencies"]["bevy"]["version"].lstrip("="),
              "rust": workspace["workspace"]["package"]["rust-version"],
              "or_version": reference["version"], "updated": config["updated"]}
    layout = (SITE / "templates/layout.html").read_text()
    result = {}
    for page in config["pages"]:
        links = []
        for item in config["pages"][:4]:
            current = ' aria-current="page"' if item["file"] == page["file"] else ""
            links.append(f'<a href="{item["file"]}"{current}>{html.escape(item["label"])}</a>')
        links.append('<a class="nav-github" href="https://github.com/cavazquez/openrailsrs">Código <span aria-hidden="true">↗</span></a>')
        variables = dict(values, title=html.escape(page["title"]), description=html.escape(page["description"], quote=True),
            canonical=config["url"]+("" if page["file"] == "index.html" else page["file"]),
            body_class="home" if page["file"] == "index.html" else "inner-page", nav="".join(links),
            content=(SITE / "src" / page["file"]).read_text())
        text = layout.replace("{{content}}", variables["content"])
        for key, value in variables.items():
            text = text.replace("{{" + key + "}}", value)
        if re.search(r"{{\w+}}", text):
            raise ValueError(f"Unresolved template in {page['file']}")
        result[SITE / page["file"]] = text
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--check", action="store_true", help="reject stale generated HTML")
    args = p.parse_args()
    for path, text in render().items():
        if args.check:
            if not path.is_file() or path.read_text() != text:
                raise SystemExit(f"Stale site: {path}; run python3 scripts/build_website.py")
        else:
            path.write_text(text)


if __name__ == "__main__":
    main()
