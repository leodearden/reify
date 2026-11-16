#!/usr/bin/env python3
"""
Guards the exclusive carve-outs from dark-factory's unlimited-holder lock rule
in dark-factory-orchestrator.yaml.

`unlimited_holder_locks` lets every lock key under its prefixes take unlimited
holders; an exact `module_overrides` entry beats the rule and keeps that key
exclusive. Dark-factory checks neither key against the other, so a mis-shaped
carve-out fails silently. Three invariants:

  1. the rule is on and carves out at least one key, so 2 and 3 are not vacuous;
  2. every override key is a lock key at lock_depth, or it never matches;
  3. every ancestor directory key of a carve-out is pinned at least as tightly,
     or a task declaring that directory co-holds with the carve-out's holder.

The matching mirrors dark-factory's shared.locking.normalize_lock and
UnlimitedHolderLocksConfig.takes_unlimited_holders, with no cross-repo import.
It reads only the yaml, never the git tree, so a docs-only commit on main
cannot redden it later.
"""

import unittest
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
ORCHESTRATOR_YAML = REPO_ROOT / "dark-factory-orchestrator.yaml"

NON_PATH_SEGMENTS = {"", ".", ".."}


def normalize_lock(path, depth):
    return "/".join(path.strip("/").split("/")[:depth])


def lies_under(key, prefix):
    return key.startswith(prefix) or key == prefix[:-1]


def ancestor_directory_keys(key, prefix):
    """Directory keys from the prefix's own directory down to key's parent."""
    segments = key.split("/")
    prefix_depth = len(prefix[:-1].split("/"))
    return ["/".join(segments[:depth]) for depth in range(prefix_depth, len(segments))]


class UnlimitedHolderLockPinsTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        config = yaml.safe_load(ORCHESTRATOR_YAML.read_text())
        rule = config.get("unlimited_holder_locks") or {}
        cls.prefixes = rule.get("prefixes") or []
        cls.overrides = config.get("module_overrides") or {}
        cls.lock_depth = config["lock_depth"]

    def carve_outs(self):
        return [
            (key, prefix)
            for key in self.overrides
            for prefix in self.prefixes
            if lies_under(key, prefix)
        ]

    def test_rule_is_on_with_exclusive_carve_outs(self):
        self.assertTrue(
            self.prefixes,
            "unlimited_holder_locks.prefixes is empty or absent, so the rule is off",
        )
        self.assertTrue(
            self.carve_outs(),
            f"no module_overrides key lies under {self.prefixes}, so nothing "
            "under the rule stays exclusive",
        )

    def test_every_pin_is_a_lock_key_at_lock_depth(self):
        for key in self.overrides:
            with self.subTest(key=key):
                normalized = normalize_lock(key, self.lock_depth)
                is_lock_key = (
                    key == normalized
                    and not set(key.split("/")) & NON_PATH_SEGMENTS
                )
                self.assertTrue(
                    is_lock_key,
                    f"module_overrides key {key!r} normalizes to {normalized!r} "
                    f"at lock_depth {self.lock_depth} (or has an empty, '.' or "
                    "'..' segment); dark-factory normalizes the requested path "
                    "before looking the key up, so this pin silently never matches",
                )

    def test_every_ancestor_directory_of_a_carve_out_is_pinned(self):
        for key, prefix in self.carve_outs():
            limit = self.overrides[key]
            for directory in ancestor_directory_keys(key, prefix):
                with self.subTest(key=key, directory=directory):
                    directory_limit = self.overrides.get(directory)
                    self.assertTrue(
                        directory_limit is not None and directory_limit <= limit,
                        f"{key!r} is pinned at {limit} but its ancestor directory "
                        f"key {directory!r} is not pinned at <= {limit}: a task "
                        f"declaring {directory!r} would take the rule's unlimited "
                        f"limit and could co-hold with the holder of {key!r}, "
                        "because dark-factory counts a hierarchical conflict only "
                        "against the requested key's limit",
                    )


if __name__ == "__main__":
    unittest.main()
