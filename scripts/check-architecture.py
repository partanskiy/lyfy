#!/usr/bin/env python3
"""Enforce the layer graph and the closed command/manager set without extra packages."""
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
CORE = ROOT / "src"
COMMANDS = {"lint", "fmt", "save", "apply"}
LAYERS = {"domain", "infrastructure", "services", "managers"}
ALLOWED = {
    "domain": {"domain"},
    "infrastructure": {"domain", "infrastructure"},
    "services": {"domain", "infrastructure", "services", "Context"},
    "managers": {"domain", "services", "Context"},
}
errors = []
service_edges = {}


def error(path, message):
    errors.append(f"{path.relative_to(ROOT)}: {message}")


for path in CORE.rglob("*.rs"):
    relative = path.relative_to(CORE)
    if relative == Path("lib.rs") or relative.parts[0] == "bin":
        continue
    layer = relative.parts[0]
    if layer not in LAYERS:
        error(path, "unexpected core layer; change the architecture decision first")
        continue
    text = path.read_text()
    # Cross-layer paths must be explicit, never hidden by root glob/group aliases.
    if re.search(r"\buse\s+crate\s*(?:::\s*\{|as\b|;)", text):
        error(path, "use explicit crate::layer paths")
    for dependency in re.findall(r"\bcrate::([A-Za-z_][A-Za-z_0-9]*)", text):
        if dependency not in ALLOWED[layer]:
            error(path, f"{layer} must not depend on {dependency}")
    if re.search(r"\b(?:clap|toml|indicatif|console|serde_yaml_ng|percent_encoding)::|std::env|\b(?:e?print(?:ln)?|dbg)!", text):
        error(path, "launch parsing, environment and terminal rendering belong to inputs/bootstrap")
    if layer != "infrastructure" and re.search(r"\b(?:alpm|alpm_sys|libarchive|libarchive3_sys)::", text):
        error(path, "native package-library bindings belong to infrastructure")
    if relative != Path("infrastructure/pacman/restore.rs") and '"/usr/bin/pacman"' in text:
        error(path, "inventory/inspection must use libalpm; only interactive restoration invokes pacman")
    if layer in {"domain", "managers"}:
        if re.search(r"\b(?:libc|walkdir|tempfile)::|std::(?:fs|io|process|os)::", text):
            error(path, "OS access belongs to infrastructure/services")
        for group in re.findall(r"use\s+std::\{(.*?)\};", text, re.S):
            if re.search(r"\b(?:fs|io|process|os)\b", group):
                error(path, "OS access belongs to infrastructure/services")
    if layer == "managers" and re.search(r"\bsuper::|\buse\s+crate\s*;", text):
        error(path, "managers must not import siblings or alias the crate root")
    if layer == "managers" and re.search(r"\bcontext\.repository\b",text):
        error(path, "managers must access persistence through named services")
    if layer != "infrastructure" and "super::" in text:
        error(path, "cross-layer dependencies must use explicit crate::layer paths")
    if "super::super::" in text or "extern crate" in text:
        error(path, "relative crate aliases are forbidden")
    if re.search(r"\bcrate::execute(?:_observed)?\b", text):
        error(path, "shared code must not invoke the application dispatcher")
    if layer == "services" and relative.name != "mod.rs":
        source = relative.parts[1].removesuffix(".rs")
        dependencies = set(re.findall(r"crate::services::([a-z_]+)", text))
        for group in re.findall(r"crate::services::\{(.*?)\}", text, re.S):
            dependencies.update(re.findall(r"\b([a-z_]+)(?=\s*(?:,|::|$))", group))
        service_edges.setdefault(source, set()).update(dependencies - {source})

managers = {p.stem for p in (CORE / "managers").glob("*.rs")} - {"mod"}
if managers != COMMANDS or any(p.is_dir() for p in (CORE / "managers").iterdir()):
    errors.append("Exactly four flat manager modules are allowed: lint, fmt, save, apply")
if {p.name for p in CORE.iterdir() if p.is_dir()} != LAYERS | {"bin"}:
    errors.append("Core directories must match the four documented layers")
request = re.search(r"pub enum Request\s*\{(.*?)\n\}", (CORE / "lib.rs").read_text(), re.S)
variants = set(re.findall(r"^    ([A-Z][A-Za-z]+)(?:,|\s*\{)", request[1], re.M)) if request else set()
if {v.lower() for v in variants} != COMMANDS:
    errors.append("The public Request enum must contain exactly four commands")


def visit(node, active, done):
    if node in active:
        errors.append("Service dependency cycle: " + " -> ".join([*active, node]))
        return
    if node in done:
        return
    for target in service_edges.get(node, set()):
        visit(target, [*active, node], done)
    done.add(node)


done = set()
for node in service_edges:
    visit(node, [], done)

manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
for package in {"clap", "toml", "config", "dotenv", "dotenvy", "indicatif", "console", "serde_yaml_ng", "percent-encoding"} & set(manifest["dependencies"]):
    if not manifest["dependencies"][package].get("optional", False):
        errors.append(f"Input dependency {package} must be optional")
if manifest.get("bin") != [{"name": "lyfy", "path": "src/bin/lyfy/main.rs", "required-features": ["cli"]}]:
    errors.append("The CLI must remain an isolated lyfy binary target")
for path in [ROOT / "src/outputs", CORE / "outputs", CORE / "inputs"]:
    if path.exists():
        error(path, "this layer is not part of the architecture")

if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)
print("Architecture boundaries passed.")
