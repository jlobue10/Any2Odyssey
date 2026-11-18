#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Check that every action the workflows use is named by a commit that its own repository still holds
# under a tag or on a branch:
#
#   GH_TOKEN=... scripts/check-action-pins.sh [FOLDER]
#
# FOLDER is the one with the workflow files (.github/workflows by default). Needs the GitHub CLI (gh).
#
# The workflows name each action by commit, so that nobody can change what runs by moving a tag. GitHub
# serves a commit under a repository's name without its being part of that repository's history: one that
# a branch has left behind, and equally one that was only ever pushed to somebody's fork of it. Up to
# 0.9.4 the workflows named dtolnay/rust-toolchain by a commit of its `stable` branch; that branch is
# rebuilt whenever the action's `master` changes, and fourteen hours after the 0.9.4 release the commit
# was on no branch and under no tag. Such a commit still runs, for as long as GitHub keeps it, and nothing
# tells it apart from a commit of a fork. This fails for such a commit, and for an action named by
# anything but a full commit id.
set -eu

folder=${1:-"$(dirname "$0")/../.github/workflows"}
[ -d "$folder" ] || { echo "usage: check-action-pins.sh [FOLDER]" >&2; exit 2; }
command -v gh >/dev/null 2>&1 || { echo "check-action-pins: the GitHub CLI (gh) is needed" >&2; exit 2; }

# One GET, tried three times: an answer that did not arrive is not an answer about the commit.
# Only the answer that arrived is passed on. gh prints the body of an error on standard output as well
# (GitHub's "You have exceeded a secondary rate limit", the page of a 502): passed on with the answer of
# the next try, it stood where the names of tags or branches are expected, and a commit that no tag and
# no branch holds was accepted as "tag {"message": ...}" whenever the first try had failed.
ask() {
    tries=0
    until answer=$(gh api "$@" 2>/dev/null); do
        tries=$((tries + 1))
        [ "$tries" -lt 3 ] || return 1
        sleep "${A2O_PIN_CHECK_WAIT:-5}"
    done
    [ -z "$answer" ] || printf '%s\n' "$answer"
}

# The words after `uses:`, without local actions (./...) and container images (docker://...).
pins=$(sed -n 's/^[[:space:]-]*uses:[[:space:]]*\([^[:space:]#]*\).*$/\1/p' "$folder"/*.yml | tr -d "\"'" |
    grep -v -e '^\./' -e '^docker://' | sort -u)
[ -n "$pins" ] || { echo "check-action-pins: no action found in $folder" >&2; exit 1; }

failed=0
for pin in $pins; do
    commit=${pin##*@}
    repository=$(printf '%s\n' "${pin%@*}" | cut -d/ -f1,2)
    if ! printf '%s\n' "$commit" | grep -Eq '^[0-9a-f]{40}$'; then
        echo "::error::$pin is not named by a full commit id."
        failed=1
        continue
    fi
    if ! tags=$(ask --paginate "repos/$repository/tags?per_page=100" \
        --jq ".[] | select(.commit.sha == \"$commit\") | .name"); then
        echo "::error::Could not read the tags of $repository."
        failed=1
        continue
    fi
    if [ -n "$tags" ]; then
        echo "ok  $pin: tag $(printf '%s' "$tags" | tr '\n' ' ')"
        continue
    fi
    # Not under a tag: on the main branch (its head or further back), or the head of another branch.
    # (A question GitHub did not answer is told apart from the answer "no": the one says to run the
    # check again, the other to change the workflows.)
    unanswered=
    branch=$(ask "repos/$repository" --jq .default_branch) || unanswered=1
    position=
    [ -z "$branch" ] || position=$(ask "repos/$repository/compare/$commit...$branch" --jq .status) || unanswered=1
    case $position in
    ahead | identical)
        echo "ok  $pin: on the branch $branch"
        continue
        ;;
    esac
    heads=$(ask "repos/$repository/commits/$commit/branches-where-head" --jq '.[].name') || unanswered=1
    if [ -n "$heads" ]; then
        echo "ok  $pin: head of the branch $(printf '%s' "$heads" | tr '\n' ' ')"
        continue
    fi
    if [ -n "$unanswered" ]; then
        echo "::error::$pin: no tag of $repository has this commit, and GitHub did not answer whether a branch has it (asked three times; it also answers so about a commit the repository does not know). Run the check again; if it says the same, name a commit that a tag or the main branch of $repository holds."
    else
        echo "::error::$pin: $repository has this commit under no tag, not on its branch $branch and at the head of no other branch. Name a commit that a tag or the main branch of $repository holds."
    fi
    failed=1
done
if [ "$failed" -ne 0 ]; then
    echo "check-action-pins: see the errors above" >&2
    exit 1
fi
