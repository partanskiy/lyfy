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


if __name__ == "__main__":
    unittest.main()
