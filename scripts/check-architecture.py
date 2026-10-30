#!/usr/bin/env python3
"""Enforce the layer graph and the closed command/operation set without extra packages."""
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
CORE = ROOT / "crates/lyfy/src"
CLI = ROOT / "crates/lyfy-cli/src"
COMMANDS = {"lint", "fmt", "save", "apply"}
LAYERS = {"domain", "infrastructure", "services", "application"}
ALLOWED = {
    "domain": {"domain"},
    "infrastructure": {"domain", "infrastructure"},
    "services": {"domain", "infrastructure", "services", "Context"},
    "application": {"domain", "services", "Context"},
}
errors = []
service_edges = {}


def error(path, message):
    errors.append(f"{path.relative_to(ROOT)}: {message}")


for path in CORE.rglob("*.rs"):
    relative = path.relative_to(CORE)
    text = path.read_text()
    if re.search(r"\b(?:clap|toml|indicatif|console|serde_yaml_ng|percent_encoding)::|std::env|\b(?:e?print(?:ln)?|dbg)!", text):
        error(path, "launch parsing, environment and terminal rendering belong to inputs/bootstrap")
    if relative in {Path("lib.rs"), Path("api.rs")}:
        continue
    layer = relative.parts[0]
    if layer not in LAYERS:
        error(path, "unexpected core layer; change the architecture decision first")
        continue
    # Cross-layer paths must be explicit, never hidden by root glob/group aliases.
    if re.search(r"\buse\s+crate\s*(?:::\s*\{|as\b|;)", text):
        error(path, "use explicit crate::layer paths")
    for dependency in re.findall(r"\bcrate::([A-Za-z_][A-Za-z_0-9]*)", text):
        if dependency not in ALLOWED[layer]:
            error(path, f"{layer} must not depend on {dependency}")
    if layer != "infrastructure" and re.search(r"\b(?:alpm|alpm_sys|libarchive|libarchive3_sys)::", text):
        error(path, "native package-library bindings belong to infrastructure")
    if relative != Path("infrastructure/pacman/restore.rs") and '"/usr/bin/pacman"' in text:
        error(path, "inventory/inspection must use libalpm; only interactive restoration invokes pacman")
    if layer in {"domain", "application"}:
        if re.search(r"\b(?:libc|walkdir|tempfile)::|std::(?:fs|io|process|os)::", text):
            error(path, "OS access belongs to infrastructure/services")
        for group in re.findall(r"use\s+std::\{(.*?)\};", text, re.S):
            if re.search(r"\b(?:fs|io|process|os)\b", group):
                error(path, "OS access belongs to infrastructure/services")
    if layer == "application" and re.search(r"\bsuper::|\buse\s+crate\s*;", text):
        error(path, "operations must not import siblings or alias the crate root")
    if layer == "application" and re.search(r"\bcontext\.repository\b",text):
        error(path, "operations must access persistence through named services")
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

operations = {p.stem for p in (CORE / "application").glob("*.rs")} - {"mod"}
if operations != COMMANDS or any(p.is_dir() for p in (CORE / "application").iterdir()):
    errors.append("Exactly four flat application operations are allowed: lint, fmt, save, apply")
declarations = [line.strip() for line in (CORE / "application/mod.rs").read_text().splitlines() if line.strip() and not line.lstrip().startswith("//")]
if len(declarations) != 4 or set(declarations) != {f"pub(crate) mod {name};" for name in COMMANDS}:
    errors.append("application/mod.rs must only declare the four operation modules")
for path in (CORE / "application").glob("*.rs"):
    if path.name != "mod.rs" and re.findall(r"^pub(?:\([^)]*\))? fn ([a-z_]+)", path.read_text(), re.M) != ["run"]:
        error(path, "an operation exposes exactly one internal run function")
    if path.name != "mod.rs" and not re.search(r"^pub\(crate\) fn run\(", path.read_text(), re.M):
        error(path, "operation run functions must be pub(crate)")
if {p.name for p in CORE.iterdir() if p.is_dir()} != LAYERS:
    errors.append("Core directories must match the four documented layers")
facade = (CORE / "lib.rs").read_text()
facade_code = "\n".join(line for line in facade.splitlines() if not line.lstrip().startswith("//"))
if set(re.findall(r"^mod ([a-z_]+);", facade_code, re.M)) != LAYERS | {"api"}:
    errors.append("lib.rs must declare only the private API and four internal layers")
if re.search(r"\bpub(?:\([^)]*\))? mod\b|\b(?:fn|struct|enum|trait|impl)\b", facade_code):
    errors.append("lib.rs must be an explicit facade, not a public layer or implementation")
for export in re.findall(r"pub use (.*?);", facade_code, re.S):
    if "*" in export or not export.startswith(("api::", "domain::")):
        errors.append("Public exports must explicitly name API/model items")
api = (CORE / "api.rs").read_text()
if set(re.findall(r"^pub fn ([a-z_]+)", api, re.M)) != {"execute", "execute_observed"}:
    errors.append("api.rs must contain exactly the two public operation entry points")
if set(re.findall(r"^pub (?:struct|enum|trait|type) ([A-Za-z]+)", api, re.M)) != {"Context", "Request", "Data", "Outcome"}:
    errors.append("Operation API types belong to the explicit api.rs contract")
if re.search(r"\b(?:libc|walkdir|tempfile|alpm|alpm_sys|libarchive|libarchive3_sys)::|std::(?:fs|io|process|os)::", api):
    errors.append("API composition must delegate OS/native mechanisms to infrastructure")
request = re.search(r"pub enum Request\s*\{(.*?)\n\}", api, re.S)
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
workspace = manifest.get("workspace", {})
if set(workspace.get("members", [])) != {"crates/lyfy", "crates/lyfy-cli"}:
    errors.append("Exactly two workspace members are allowed: lyfy and lyfy-cli")
if "package" in manifest or workspace.get("resolver") != "3":
    errors.append("The root must be a virtual workspace using resolver 3")
if {p.name for p in (ROOT / "crates").iterdir() if p.is_dir()} != {"lyfy", "lyfy-cli"}:
    errors.append("Unexpected workspace package directory")
core_manifest = tomllib.loads((CORE.parent / "Cargo.toml").read_text())
cli_manifest = tomllib.loads((CLI.parent / "Cargo.toml").read_text())
if core_manifest.get("package", {}).get("name") != "lyfy" or core_manifest.get("lib") != {"name": "lyfy", "path": "src/lib.rs"}:
    errors.append("lyfy must be the library package")
if core_manifest.get("bin") or "cli" in core_manifest.get("features", {}):
    errors.append("The library cannot contain a binary target or cli feature")
if cli_manifest.get("package", {}).get("autolib") is not False:
    errors.append("CLI must disable automatic library discovery")
if cli_manifest.get("package", {}).get("name") != "lyfy-cli" or cli_manifest.get("bin") != [{"name": "lyfy", "path": "src/main.rs"}] or "lib" in cli_manifest:
    errors.append("lyfy-cli must contain exactly the lyfy binary target")
for member in (core_manifest, cli_manifest):
    if member.get("package", {}).get("autobins") is not False:
        errors.append("Workspace packages must disable automatic binary discovery")
if cli_manifest.get("dependencies", {}).get("lyfy") != {"path": "../lyfy"}:
    errors.append("The CLI must explicitly depend on the local lyfy library")


def dependencies(member):
    tables = [member, *member.get("target", {}).values()]
    for table in tables:
        for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
            for name, spec in table.get(kind, {}).items():
                spec = spec if isinstance(spec, dict) else {}
                if spec.get("workspace"):
                    inherited = workspace.get("dependencies", {}).get(name, {})
                    if isinstance(inherited, dict):
                        spec = {**inherited, **spec}
                yield spec.get("package", name), spec


input_packages = {"lyfy-cli", "clap", "toml", "config", "dotenv", "dotenvy", "indicatif", "console", "serde_yaml_ng", "percent-encoding"}
for name, spec in dependencies(core_manifest):
    if name in input_packages or ("path" in spec and (CORE.parent / spec["path"]).resolve() == CLI.parent):
        errors.append(f"Library dependency {name} belongs to launch inputs")
for name, _ in dependencies(cli_manifest):
    if name in {"alpm", "alpm-sys", "libarchive", "libarchive3-sys"}:
        errors.append(f"Native package dependency {name} belongs to core infrastructure")
for path in CLI.rglob("*.rs"):
    text = path.read_text()
    if re.search(r"\blyfy::(?:api|application|domain|infrastructure|services)\b|\b(?:alpm|alpm_sys|libarchive|libarchive3_sys)::", text):
        error(path, "CLI must use only the public core API, not infrastructure bindings")
if {p.name for p in CLI.iterdir() if p.is_dir()} != {"inputs"}:
    errors.append("CLI launch modules belong to inputs")
for path in [ROOT / "src", ROOT / "tests", CORE / "outputs", CORE / "inputs", CLI / "outputs"]:
    if path.exists():
        error(path, "this directory is not part of the workspace architecture")

if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)
print("Architecture boundaries passed.")
