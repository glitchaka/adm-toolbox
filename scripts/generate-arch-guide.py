#!/usr/bin/env python3
import hashlib
import io
import json
import os
import pathlib
import re
import shutil
import tarfile
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
TUTORIALS = ROOT / "tutorials"
GUIDE = TUTORIALS / "guide-index.json"
MIRROR = "https://geo.mirror.pkgbuild.com"
REPOS = ("core", "extra", "multilib")
MAX_INDEX = 2 * 1024 * 1024


def fields(text: str):
    out = {}
    key = None
    for raw in text.splitlines():
        line = raw.rstrip("\n")
        if line.startswith("%") and line.endswith("%"):
            key = line[1:-1]
            out.setdefault(key, [])
        elif key and line:
            out[key].append(line)
    return out


def one(meta, key, default=""):
    values = meta.get(key, [])
    return values[0] if values else default


def clean_dep(value: str):
    # Keep Arch's dependency expression, only normalize whitespace.
    return " ".join(value.split())


def topic_for(name: str):
    # guide .8's numbered-section extractor tokenizes punctuation; give it the
    # same token stream so package names such as python-foo still match.
    return " ".join(re.findall(r"[A-Za-z0-9]+", name)) or name


def shard_for(name: str):
    return hashlib.sha256(name.encode("utf-8")).hexdigest()[:2]


def fetch_repo(repo: str):
    url = f"{MIRROR}/{repo}/os/x86_64/{repo}.db"
    print(f"fetch {url}")
    with urllib.request.urlopen(url, timeout=45) as response:
        raw = response.read()
    packages = []
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:*") as archive:
        for member in archive.getmembers():
            if not member.isfile() or not member.name.endswith("/desc"):
                continue
            fileobj = archive.extractfile(member)
            if not fileobj:
                continue
            meta = fields(fileobj.read().decode("utf-8", "replace"))
            name = one(meta, "NAME")
            if not name:
                continue
            packages.append({
                "name": name,
                "repo": repo,
                "version": one(meta, "VERSION"),
                "arch": one(meta, "ARCH"),
                "desc": one(meta, "DESC"),
                "url": one(meta, "URL"),
                "licenses": meta.get("LICENSE", []),
                "depends": [clean_dep(x) for x in meta.get("DEPENDS", [])],
                "optdepends": [clean_dep(x) for x in meta.get("OPTDEPENDS", [])],
                "makedepends": [clean_dep(x) for x in meta.get("MAKEDEPENDS", [])],
                "checkdepends": [clean_dep(x) for x in meta.get("CHECKDEPENDS", [])],
                "provides": [clean_dep(x) for x in meta.get("PROVIDES", [])],
            })
    return packages


def format_list(label, values):
    if not values:
        return f"{label}: —"
    return f"{label}: " + ", ".join(values)


def main():
    with GUIDE.open("r", encoding="utf-8") as fh:
        guide = json.load(fh)

    # Replace any previous hand-written Arch seed completely.
    guide["areas"] = [area for area in guide.get("areas", []) if area.get("id") != "arch"]
    guide["entries"] = [entry for entry in guide.get("entries", []) if entry.get("area") != "arch"]

    packages = []
    for repo in REPOS:
        packages.extend(fetch_repo(repo))

    # Stable repos may contain the same package name in different repos only in
    # unusual transitions. Prefer core > extra > multilib deterministically.
    rank = {"core": 0, "extra": 1, "multilib": 2}
    selected = {}
    for package in sorted(packages, key=lambda p: (p["name"].lower(), rank[p["repo"]])):
        selected.setdefault(package["name"], package)
    packages = sorted(selected.values(), key=lambda p: p["name"].lower())

    # Keep this separate from normal SST documentation: one first-level area,
    # positioned after the ordinary guide areas, backed by generated package data.
    guide["areas"].append({
        "id": "arch",
        "name": "Arch / Portabilidad",
        "summary": f"Catálogo oficial Arch para revisión ({len(packages)} paquetes).",
    })

    shard_lines = {}
    shard_counts = {}
    entries = []
    for package in packages:
        name = package["name"]
        shard = shard_for(name)
        number = shard_counts.get(shard, 0) + 1
        shard_counts[shard] = number
        body = [
            f"{number}. {name}",
            f"Paquete: {name}",
            f"Repositorio: {package['repo']}",
            f"Versión: {package['version']}",
            f"Arquitectura: {package['arch']}",
            f"Descripción: {package['desc'] or '—'}",
            f"Upstream: {package['url'] or '—'}",
            format_list("Licencia", package["licenses"]),
            format_list("Dependencias", package["depends"]),
            format_list("Opcionales", package["optdepends"]),
            format_list("Compilación", package["makedepends"]),
            format_list("Pruebas", package["checkdepends"]),
            format_list("Proporciona", package["provides"]),
            "Estado SST: [ ]",
            "",
        ]
        shard_lines.setdefault(shard, []).extend(body)
        entries.append({
            "name": name,
            "area": "arch",
            "summary": package["version"],
            "manual": {
                "file": f"arch-{shard}",
                "section": "unix",
                "topic": topic_for(name),
            },
        })

    guide["entries"].extend(entries)

    # Compact JSON is intentional: SST .8 limits the remote catalog to 2 MiB.
    encoded = json.dumps(guide, ensure_ascii=False, separators=(",", ":")) + "\n"
    size = len(encoded.encode("utf-8"))
    if size > MAX_INDEX:
        raise SystemExit(f"guide-index.json would be {size} bytes (> {MAX_INDEX})")
    GUIDE.write_text(encoded, encoding="utf-8")

    # Remove previous generated shards and recreate them. 256 shards keep every
    # file well below SST's 512 KiB per-manual limit.
    for old in TUTORIALS.glob("arch-[0-9a-f][0-9a-f].txt"):
        old.unlink()
    for shard, lines in shard_lines.items():
        path = TUTORIALS / f"arch-{shard}.txt"
        path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")
        if path.stat().st_size > 512 * 1024:
            raise SystemExit(f"{path.name} exceeds SST manual limit")

    # Remove obsolete seed files/index from the three-item mock catalog.
    for obsolete in ("arch-index.json", "arch-ripgrep.txt", "arch-htop.txt", "arch-tcpdump.txt"):
        path = TUTORIALS / obsolete
        if path.exists():
            path.unlink()

    print(f"generated {len(packages)} Arch packages; index={size} bytes; shards={len(shard_lines)}")


if __name__ == "__main__":
    main()
