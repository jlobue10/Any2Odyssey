#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""scripts/check-action-pins.sh against a stand-in for the GitHub CLI: which commits it accepts and refuses.

The real check asks GitHub about the actions the workflows name, so in CI it only ever shows that today's
commits are accepted. Here a `gh` that answers from files stands in, and the refusals are run too: a commit
on no branch and under no tag (what the one 0.9.4 was built with became the day after), a tag or a short id
in place of a commit, an answer that never arrives, and one that arrives only at the second try. A2O_TEST_SHELL runs the script
under another shell (dash, "busybox sh").
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check-action-pins.sh"
SHELL = os.environ.get("A2O_TEST_SHELL", "sh").split()

TAGGED = "1a" * 20
ON_MAIN = "2b" * 20
BRANCH_HEAD = "3c" * 20
LEFT_BEHIND = "4d" * 20

# Answers what the script asks, from files under $STAND_IN: tags/<owner>__<repo> ("<commit> <tag>" lines),
# default/<owner>__<repo>, compare/<owner>__<repo>__<commit> and heads/<owner>__<repo>__<commit>.
# A repository without a tags file is one GitHub does not answer for. With a file fail-first, the first
# time each question is asked it fails the way the real gh fails on an HTTP error: the body of the error on
# standard output, exit 1. A file unanswered holds a word; a question with that word in it always fails so.
# A file unanswered-whole holds one whole question, which always fails so.
GH = r"""#!/bin/sh
[ "$1" = api ] || exit 2
shift
endpoint= query=
while [ $# -gt 0 ]; do
    case $1 in
    --paginate) ;;
    --jq) shift; query=$1 ;;
    *) endpoint=$1 ;;
    esac
    shift
done
echo "$endpoint" >> "$STAND_IN/asked"
refuse() {
    printf '%s' '{"message":"You have exceeded a secondary rate limit.","documentation_url":"https://docs.github.com/rest","status":"403"}'
    echo "gh: You have exceeded a secondary rate limit (HTTP 403)" >&2
    exit 1
}
if [ -f "$STAND_IN/fail-first" ]; then
    once=$STAND_IN/failed-$(printf '%s' "$endpoint" | tr -c 'A-Za-z0-9' _)
    [ -e "$once" ] || { : > "$once"; refuse; }
fi
if [ -f "$STAND_IN/unanswered" ]; then
    case $endpoint in *"$(cat "$STAND_IN/unanswered")"*) refuse ;; esac
fi
if [ -f "$STAND_IN/unanswered-whole" ] && [ "$endpoint" = "$(cat "$STAND_IN/unanswered-whole")" ]; then
    refuse
fi
path=${endpoint#repos/}
name=$(printf '%s\n' "$path" | cut -d/ -f1,2 | sed 's|/|__|')
rest=$(printf '%s\n' "$path" | cut -d/ -f3-)
case $rest in
'tags?per_page=100')
    [ -f "$STAND_IN/tags/$name" ] || exit 1
    commit=$(printf '%s\n' "$query" | grep -Eo '[0-9a-f]{40}')
    sed -n "s/^$commit //p" "$STAND_IN/tags/$name"
    ;;
'') cat "$STAND_IN/default/$name" ;;
compare/*)
    commit=${rest#compare/}
    cat "$STAND_IN/compare/${name}__${commit%%...*}" 2>/dev/null || echo diverged
    ;;
commits/*/branches-where-head)
    commit=${rest#commits/}
    cat "$STAND_IN/heads/${name}__${commit%%/*}" 2>/dev/null || true
    ;;
*) exit 1 ;;
esac
"""


class CheckActionPinsTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.stand_in = self.root / "github"
        for folder in ["tags", "default", "compare", "heads"]:
            (self.stand_in / folder).mkdir(parents=True)
        tools = self.root / "tools"
        tools.mkdir()
        (tools / "gh").write_text(GH)
        (tools / "gh").chmod(0o755)
        self.workflows = self.root / "workflows"
        self.workflows.mkdir()
        self.env = dict(os.environ, PATH=f"{tools}{os.pathsep}{os.environ['PATH']}",
                        STAND_IN=str(self.stand_in), A2O_PIN_CHECK_WAIT="0")
        # One repository with a tagged commit, one on the main branch and one at the head of another.
        self.answer("tags", "one__action", f"{TAGGED} v4.4.0\n{TAGGED} v4\n{'9' * 40} v5\n")
        self.answer("default", "one__action", "main\n")
        self.answer("compare", f"one__action__{ON_MAIN}", "ahead\n")
        self.answer("heads", f"one__action__{BRANCH_HEAD}", "releases/v3\n")

    def answer(self, kind, name, text):
        (self.stand_in / kind / name).write_text(text)

    def check(self, *uses):
        (self.workflows / "ci.yml").write_text(
            "jobs:\n  one:\n    steps:\n" + "".join(f"      {line}\n" for line in uses))
        return subprocess.run([*SHELL, str(SCRIPT), str(self.workflows)], env=self.env, text=True,
                              capture_output=True, timeout=60, stdin=subprocess.DEVNULL)

    def test_commits_of_a_tag_the_main_branch_and_a_branch_head_are_accepted(self):
        result = self.check(f"- uses: one/action@{TAGGED} # v4",
                            f"- uses: one/action@{ON_MAIN} # commit on main",
                            f"- name: named first\n        uses: one/action/in/a/folder@{BRANCH_HEAD}")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"ok  one/action@{TAGGED}: tag v4.4.0 v4", result.stdout)
        self.assertIn(f"ok  one/action@{ON_MAIN}: on the branch main", result.stdout)
        self.assertIn(f"ok  one/action/in/a/folder@{BRANCH_HEAD}: head of the branch releases/v3", result.stdout)

    def test_a_commit_that_no_tag_and_no_branch_holds_is_refused(self):
        # What a rebuilt branch leaves behind, and what a commit pushed only to a fork looks like.
        result = self.check(f"- uses: one/action@{TAGGED} # v4", f"- uses: one/action@{LEFT_BEHIND} # stable")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f"::error::one/action@{LEFT_BEHIND}: one/action has this commit under no tag", result.stdout)
        self.assertIn(f"ok  one/action@{TAGGED}", result.stdout)

    def test_a_tag_a_branch_or_a_short_id_in_place_of_a_commit_is_refused(self):
        for reference in ["v4", "main", TAGGED[:12], TAGGED.upper()]:
            with self.subTest(reference=reference):
                result = self.check(f"- uses: one/action@{reference}")
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(f"::error::one/action@{reference} is not named by a full commit id", result.stdout)

    def test_an_answer_that_does_not_arrive_is_not_taken_for_a_yes(self):
        result = self.check(f"- uses: nobody/answers@{TAGGED}")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("::error::Could not read the tags of nobody/answers", result.stdout)
        # Asked three times before giving up.
        asked = (self.stand_in / "asked").read_text().splitlines()
        self.assertEqual(asked.count("repos/nobody/answers/tags?per_page=100"), 3)

    def test_the_body_of_a_failed_try_is_not_taken_for_the_name_of_a_tag_or_a_branch(self):
        # gh prints what GitHub answered to a failed request on standard output. Up to 0.9.5 that went
        # into the answer together with the next try's: after one failed try a commit that no tag and no
        # branch holds was accepted, as under the tag {"message":"You have exceeded ...
        (self.stand_in / "fail-first").touch()
        result = self.check(f"- uses: one/action@{LEFT_BEHIND} # stable")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f"::error::one/action@{LEFT_BEHIND}: one/action has this commit under no tag", result.stdout)
        self.assertNotIn("ok  ", result.stdout)
        # The commits that are held are still accepted, and what is printed are the names alone.
        for failed in self.stand_in.glob("failed-*"):
            failed.unlink()
        result = self.check(f"- uses: one/action@{TAGGED}", f"- uses: one/action@{ON_MAIN}",
                            f"- uses: one/action@{BRANCH_HEAD}")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"ok  one/action@{TAGGED}: tag v4.4.0 v4\n", result.stdout)
        self.assertIn(f"ok  one/action@{ON_MAIN}: on the branch main\n", result.stdout)
        self.assertIn(f"ok  one/action@{BRANCH_HEAD}: head of the branch releases/v3\n", result.stdout)
        self.assertNotIn("message", result.stdout)

    def test_a_question_about_the_branches_that_gets_no_answer_is_said_as_that(self):
        # GitHub answered for the tags and then not any more: the commit is refused (nobody said it is
        # held), but not with the sentence that tells to change the workflows.
        for question in ["compare", "branches-where-head"]:
            with self.subTest(question=question):
                (self.stand_in / "unanswered").write_text(question)
                result = self.check(f"- uses: one/action@{LEFT_BEHIND}")
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("GitHub did not answer whether a branch has it", result.stdout)
                self.assertNotIn("not on its branch", result.stdout)
        # A commit of the main branch is still found when only the last question goes unanswered.
        result = self.check(f"- uses: one/action@{ON_MAIN}")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        # The first question about the branches, which of them is the main one. Without its answer the
        # commit cannot be looked for on that branch: that is "no answer" as well, not "no", also for a
        # commit the main branch holds.
        (self.stand_in / "unanswered").unlink()
        (self.stand_in / "unanswered-whole").write_text("repos/one/action")
        for commit in [LEFT_BEHIND, ON_MAIN]:
            with self.subTest(question="the main branch", commit=commit):
                result = self.check(f"- uses: one/action@{commit}")
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("GitHub did not answer whether a branch has it", result.stdout)
                self.assertNotIn("not on its branch", result.stdout)
        # The head of another branch is found all the same.
        result = self.check(f"- uses: one/action@{BRANCH_HEAD}")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_local_actions_images_and_comments_are_not_asked_about(self):
        result = self.check("- uses: ./.github/actions/local", "- uses: docker://alpine:3",
                            f"# - uses: one/action@{LEFT_BEHIND}", f"- uses: 'one/action@{TAGGED}' # quoted")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(result.stdout.count("ok  "), 1, result.stdout)

    def test_a_folder_without_any_action_is_an_error_not_a_pass(self):
        result = self.check("- run: true")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("no action found", result.stderr)


if __name__ == "__main__":
    unittest.main()
