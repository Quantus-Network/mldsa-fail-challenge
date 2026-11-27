"""Regression checks for publish.py. Run: python3 .github/scripts/publish_test.py"""

import importlib.util
import io
import json
import os
import tempfile
import unittest
from pathlib import Path

os.environ["GITHUB_ACTIONS"] = "true"
os.environ["GITHUB_REPOSITORY"] = "Quantus-Network/mldsa-fail-challenge"

spec = importlib.util.spec_from_file_location("publish", Path(__file__).with_name("publish.py"))
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)


class GitHub:
	def __init__(self):
		self.closed = []
		self.fail_push = False

	def run(self, args, **kwargs):
		class Result:
			returncode = 0
			stdout = "OPEN\n"
			stderr = ""

		if args[:3] == ["git", "diff", "--cached"]:
			Result.returncode = 1
		elif self.fail_push and args[:3] == ["git", "push", "origin"]:
			Result.returncode = 1
			Result.stderr = "push failed"
		elif args[:4] == ["gh", "api", "-X", "PATCH"]:
			self.closed.append(int(args[4].rsplit("/", 1)[-1]))
		return Result()


def artifact(root, run_id, pull_request, score):
	art = root / f"artifact-{run_id}"
	art.mkdir()
	(art / "meta.json").write_text(json.dumps({
		"run_id": run_id,
		"commit": f"{run_id:040x}",
		"pr": pull_request,
		"started": "t",
		"finished": "t",
		"budget_secs": 1,
		"runner": "r",
		"drand": {"round": 1},
		"seed": "s",
		"bench_exit": 0,
		"smoke": False,
		"run_url": "http://example",
	}))
	(art / "score.json").write_text(json.dumps({
		"status": "accepted",
		"score_bits": score,
		"first_over_gh": 2.0,
		"effective_block_size": 3.0,
	}))
	(art / "report.json").write_text("{}")
	(art / "SUBMISSION.toml").write_text("")
	pr_path = root / f"pr-{run_id}.json"
	pr_path.write_text(json.dumps({"title": "t", "author": {"login": "a"}}))
	return art, pr_path


class CloseWhilePromotionIsPending(unittest.TestCase):
	def setUp(self):
		self.github = GitHub()
		publish.subprocess.run = self.github.run
		self.root = Path(tempfile.mkdtemp())
		self.records = self.root / "records"
		self.records.mkdir()

	def fold(self, run_id, pull_request, score):
		art, pr_path = artifact(self.root, run_id, pull_request, score)
		saved = publish.sys.stdout
		publish.sys.stdout = sink = io.StringIO()
		try:
			publish.fold(art, self.records, pr_path, 0.0)
		finally:
			publish.sys.stdout = saved
		return json.loads(sink.getvalue())

	def test_a_later_run_of_the_same_pull_request_leaves_it_open(self):
		self.fold(101, 8, 100.0)
		decision = self.fold(102, 8, 1.0)
		self.assertTrue(decision["promote"])
		self.assertEqual(decision["record_id"], "101")
		index = json.loads((self.records / "index.json").read_text())
		self.assertEqual(index["record"]["id"], "101")
		self.assertEqual(index["record"]["promotion"], "pending")
		self.assertEqual(index["record"]["pr"], 8)
		self.assertNotIn(8, self.github.closed)

	def test_a_different_pull_request_still_closes(self):
		self.fold(101, 8, 100.0)
		self.fold(201, 9, 1.0)
		self.assertNotIn(8, self.github.closed)
		self.assertIn(9, self.github.closed)
		index = json.loads((self.records / "index.json").read_text())
		self.assertEqual(index["record"]["id"], "101")
		self.assertEqual(index["record"]["promotion"], "pending")


if __name__ == "__main__":
	unittest.main()
