#!/usr/bin/env python3
"""Exercise workspace boundary guards using private temporary source copies."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class WorkspaceBoundaries(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="lyfy-architecture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "crates", self.root / "crates")
        shutil.copyfile(ROOT / "Cargo.toml", self.root / "Cargo.toml")
        (self.root / "scripts").mkdir()
        shutil.copyfile(ROOT / "scripts/check-architecture.py", self.root / "scripts/check-architecture.py")

    def append(self, relative, text):
        with (self.root / relative).open("a") as target:
            target.write("\n" + text + "\n")

    def check(self, expected=None):
        result = subprocess.run([sys.executable, str(self.root / "scripts/check-architecture.py")],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1 if expected else 0, result.stderr)
        if expected:
            self.assertIn(expected, result.stderr)

    def test_current_workspace_passes(self):
        self.check()

    def test_core_cannot_add_a_direct_launch_dependency(self):
        self.append("crates/lyfy/Cargo.toml", 'clap = "4"')
        self.check("Library dependency clap belongs to launch inputs")

    def test_core_cannot_hide_a_dependency_behind_a_workspace_alias(self):
        self.append("Cargo.toml", 'render = { package = "indicatif", version = "0.18" }')
        self.append("crates/lyfy/Cargo.toml", 'render.workspace = true')
        self.check("Library dependency indicatif belongs to launch inputs")

    def test_core_cannot_add_platform_specific_launch_dependencies(self):
        self.append("crates/lyfy/Cargo.toml", '[target.\'cfg(unix)\'.dependencies]\nparser = { package = "toml", version = "0.8" }')
        self.check("Library dependency toml belongs to launch inputs")

    def test_core_cannot_depend_back_on_the_cli_for_tests(self):
        self.append("crates/lyfy/Cargo.toml", '[dev-dependencies]\nlauncher = { path = "../lyfy-cli" }')
        self.check("Library dependency launcher belongs to launch inputs")

    def test_cli_cannot_bypass_core_for_native_package_access(self):
        self.append("crates/lyfy-cli/Cargo.toml", 'native = { package = "alpm", version = "5" }')
        self.check("Native package dependency alpm belongs to core infrastructure")

    def test_cli_feature_cannot_return_to_the_library(self):
        self.append("crates/lyfy/Cargo.toml", '[features]\ncli = []')
        self.check("The library cannot contain a binary target or cli feature")

    def test_a_third_workspace_member_requires_an_architecture_change(self):
        manifest = self.root / "Cargo.toml"
        manifest.write_text(manifest.read_text().replace('"crates/lyfy-cli"]', '"crates/lyfy-cli", "crates/settings"]'))
        self.check("Exactly two workspace members are allowed")

    def test_launch_parsing_cannot_move_into_core_services(self):
        self.append("crates/lyfy/src/services/files.rs", "use toml::Value;")
        self.check("launch parsing, environment and terminal rendering belong to inputs/bootstrap")

    def test_the_library_entry_point_cannot_render_to_the_terminal(self):
        self.append("crates/lyfy/src/lib.rs", 'fn render() { println!("bad"); }')
        self.check("launch parsing, environment and terminal rendering belong to inputs/bootstrap")

    def test_operation_entry_points_cannot_become_public(self):
        path = self.root / "crates/lyfy/src/application/fmt.rs"
        path.write_text(path.read_text().replace("pub(crate) fn run", "pub fn run"))
        self.check("operation run functions must be pub(crate)")

    def test_operations_cannot_import_each_other(self):
        self.append("crates/lyfy/src/application/fmt.rs", "use crate::application::save;")
        self.check("application must not depend on application")

    def test_services_cannot_call_the_dispatcher_through_its_module(self):
        self.append("crates/lyfy/src/services/files.rs", "use crate::api::execute;")
        self.check("services must not depend on api")

    def test_internal_domain_cannot_become_a_public_namespace(self):
        path = self.root / "crates/lyfy/src/lib.rs"
        path.write_text(path.read_text().replace("mod domain;", "pub mod domain;"))
        self.check("lib.rs must be an explicit facade")

    def test_public_model_exports_cannot_use_globs(self):
        self.append("crates/lyfy/src/lib.rs", "pub use domain::rules::*;")
        self.check("Public exports must explicitly name API/model items")

    def test_a_new_public_operation_function_requires_a_contract_change(self):
        self.append("crates/lyfy/src/api.rs", "pub fn another_operation() {}")
        self.check("api.rs must contain exactly the two public operation entry points")

    def test_application_module_cannot_hide_an_extra_dispatcher(self):
        self.append("crates/lyfy/src/application/mod.rs", "pub(crate) fn dispatch_extra() {}")
        self.check("application/mod.rs must only declare the four operation modules")


if __name__ == "__main__":
    unittest.main()
