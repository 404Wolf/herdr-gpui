"""Run: python3 -m unittest discover -s scripts/release/tests -v"""

import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DRIVER = ROOT / "scripts/git/merge-cargo-lock.sh"


class MergeDriverTests(unittest.TestCase):
    """Only a merge may defer the lockfile to the commit hooks."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="herdr-lock-driver-")
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.git("init", "--quiet", "--initial-branch=main")
        for key, value in (("user.name", "Test"), ("user.email", "test@example.com"),
                           ("commit.gpgsign", "false"),
                           ("merge.cargo-lock.driver", f"bash {DRIVER} %O %A %B")):
            self.git("config", key, value)
        (self.work / ".gitattributes").write_text("Cargo.lock merge=cargo-lock\n")
        self.commit_lock("a\nbase\nc\n", "base")
        self.git("checkout", "--quiet", "-b", "theirs")
        self.commit_lock("a\ntheirs\nc\n", "theirs")
        self.git("checkout", "--quiet", "main")
        self.commit_lock("a\nours\nc\n", "ours")

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.work), *args],
                              capture_output=True, text=True, timeout=60)

    def commit_lock(self, text, message):
        (self.work / "Cargo.lock").write_text(text)
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", message)

    def marker(self):
        path = self.git("rev-parse", "--git-path", "herdr-cargo-lock-stale").stdout.strip()
        return (self.work / path).exists()

    def unmerged(self):
        return self.git("diff", "--name-only", "--diff-filter=U").stdout.split()

    def test_a_merge_keeps_ours_and_leaves_the_rebuild_to_the_hooks(self):
        result = self.git("merge", "--no-commit", "theirs")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.unmerged(), [])
        self.assertEqual((self.work / "Cargo.lock").read_text(), "a\nours\nc\n")
        self.assertTrue(self.marker())

    def test_a_cherry_pick_stops_with_the_lockfile_conflicted(self):
        result = self.git("cherry-pick", "theirs")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("git add Cargo.lock", result.stderr)
        self.assertEqual(self.unmerged(), ["Cargo.lock"])
        self.assertEqual((self.work / "Cargo.lock").read_text(), "a\nours\nc\n")
        self.assertFalse(self.marker())

    def test_a_rebase_stops_with_the_lockfile_conflicted(self):
        self.git("checkout", "--quiet", "theirs")
        result = self.git("rebase", "main")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.unmerged(), ["Cargo.lock"])
        self.assertFalse(self.marker())


if __name__ == "__main__":
    unittest.main()
