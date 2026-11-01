#!/usr/bin/env python3
import hashlib
import io
import json
import pathlib
import re
import tarfile
import urllib.request
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parents[1]
TUTORIALS = ROOT / "tutorials"
GUIDE = TUTORIALS / "guide-index.json"
MIRROR = "https://geo.mirror.pkgbuild.com"
REPOS = ("core", "extra", "multilib")
MAX_INDEX = 2 * 1024 * 1024
MAX_MANUAL = 512 * 1024


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
    return " ".join(value.split())


def topic_for(name: str):
    return " ".join(re.findall(r"[A-Za-z0-9]+", name)) or name


def topic_tokens(topic: str):
    return topic.lower().split()


def shard_for(name: str):
    return hashlib.sha256(name.encode("utf-8")).hexdigest()[:2]


def dedicated_file(name: str):
    digest = hashlib.sha256(name.encode("utf-8")).hexdigest()[:12]
    return f"arch-p-{digest}"


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


def build_body(package, number):
    name = package["name"]
    topic = package["topic"]
    return [
        f"{number}. {topic}",
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


def main():
    with GUIDE.open("r", encoding="utf-8") as fh:
        guide = json.load(fh)

    guide["areas"] = [area for area in guide.get("areas", []) if area.get("id") != "arch"]
    guide["entries"] = [entry for entry in guide.get("entries", []) if entry.get("area") != "arch"]

    reserved = set()
    for entry in guide.get("entries", []):
        name = str(entry.get("name", "")).strip().lower()
        if name:
            reserved.add(name)
        for alias in entry.get("aliases", []):
            alias = str(alias).strip().lower()
            if alias:
                reserved.add(alias)

    packages = []
    for repo in REPOS:
        packages.extend(fetch_repo(repo))

    rank = {"core": 0, "extra": 1, "multilib": 2}
    selected = {}
    for package in sorted(packages, key=lambda p: (p["name"].lower(), rank[p["repo"]])):
        selected.setdefault(package["name"], package)
    packages = sorted(selected.values(), key=lambda p: p["name"].lower())

    for package in packages:
        package["topic"] = topic_for(package["name"])
        package["base_shard"] = shard_for(package["name"])

    # SST .8's extractor matches a section when every requested topic token is
    # present in the heading, so a short package can accidentally resolve to an
    # earlier longer package in the same shard. Detect exactly that condition
    # and isolate only the ambiguous package in its own tiny manual file.
    groups = defaultdict(list)
    for package in packages:
        groups[package["base_shard"]].append(package)

    isolated = set()
    for shard_packages in groups.values():
        earlier = []
        for package in shard_packages:
            wanted = set(topic_tokens(package["topic"]))
            for prior in earlier:
                prior_tokens = set(topic_tokens(prior["topic"]))
                if wanted.issubset(prior_tokens):
                    isolated.add(package["name"])
                    break
            earlier.append(package)

    guide["areas"].append({
        "id": "arch",
        "name": "Arch / Portabilidad",
        "summary": f"Catálogo oficial Arch para revisión ({len(packages)} paquetes).",
    })

    file_packages = defaultdict(list)
    for package in packages:
        if package["name"] in isolated:
            manual_file = dedicated_file(package["name"])
        else:
            manual_file = f"arch-{package['base_shard']}"
        package["manual_file"] = manual_file
        file_packages[manual_file].append(package)

    entries = []
    collisions = []
    for package in packages:
        name = package["name"]
        item = {
            "name": name,
            "area": "arch",
            "summary": package["version"],
            "manual": {
                "file": package["manual_file"],
                "section": "unix",
                "topic": package["topic"],
            },
        }
        if name.lower() in reserved:
            item["name"] = f"{name} [Arch]"
            item["aliases"] = [f"arch {name}"]
            item["package"] = name
            collisions.append(name)
        entries.append(item)

    guide["entries"].extend(entries)

    encoded = json.dumps(guide, ensure_ascii=False, separators=(",", ":")) + "\n"
    size = len(encoded.encode("utf-8"))
    if size > MAX_INDEX:
        raise SystemExit(f"guide-index.json would be {size} bytes (> {MAX_INDEX})")

    # Build all manuals in memory and validate every size BEFORE mutating the
    # live guide or deleting the previous shards. A generation failure therefore
    # cannot partially publish a broken catalog.
    rendered = {}
    for manual_file, members in file_packages.items():
        lines = []
        for number, package in enumerate(members, 1):
            lines.extend(build_body(package, number))
        text = "\n".join(lines).rstrip() + "\n"
        if len(text.encode("utf-8")) > MAX_MANUAL:
            raise SystemExit(f"{manual_file}.txt exceeds SST manual limit")
        rendered[manual_file] = text

    GUIDE.write_text(encoded, encoding="utf-8")

    for old in TUTORIALS.glob("arch-[0-9a-f][0-9a-f].txt"):
        old.unlink()
    for old in TUTORIALS.glob("arch-p-*.txt"):
        old.unlink()
    for manual_file, text in rendered.items():
        (TUTORIALS / f"{manual_file}.txt").write_text(text, encoding="utf-8")

    for obsolete in ("arch-index.json", "arch-ripgrep.txt", "arch-htop.txt", "arch-tcpdump.txt"):
        path = TUTORIALS / obsolete
        if path.exists():
            path.unlink()

    print(
        f"generated {len(packages)} Arch packages; index={size} bytes; "
        f"manuals={len(rendered)}; isolated={len(isolated)}; collisions={len(collisions)}"
    )
    if isolated:
        print("isolated ambiguous topics: " + ", ".join(sorted(isolated)))
    if collisions:
        print("disambiguated SST lookup collisions: " + ", ".join(collisions))


if __name__ == "__main__":
    main()
