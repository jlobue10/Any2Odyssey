#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
# Installs packages with apt-get on a runner of the workflows, without staying on a mirror that is slow.
#
#   scripts/apt-install.sh [--update] PACKAGE...
#
# The workflows ran "sudo apt-get update" and "sudo apt-get install" as they are. apt-get gives up on a
# mirror that does not answer, but not on one that answers slowly: in the first attempt of run 44778450011
# the step that installs the build dependencies took 17.5 minutes where it takes under one, and the step
# that installs MinGW was still downloading when the job's 45 minutes were up. Nothing had failed, so
# nothing was tried again, and the job was cancelled without a line that said why.
#
# Here the parts that use the network (the package lists with --update, then the packages' files) each get
# a time limit and three tries. What a try has fetched is kept, so the next one goes on where it stopped.
# Before the files are tried again the lists are fetched once more (lists_again below says why).
# When the third try has not finished either, the script fails, and says which part it was. Only then are
# the packages installed, from the files that are on the disk by now: that part is never stopped half-way.
# The step's timeout-minutes in the workflow is the bound on all of it.
set -eu

UPDATE=
if [ "${1:-}" = --update ]; then UPDATE=1; shift; fi
[ $# -ge 1 ] || { echo "usage: apt-install.sh [--update] PACKAGE..." >&2; exit 2; }

TRIES=3
# Seconds between two tries (scripts/tests/test-apt-install.py sets it to nothing).
PAUSE=${A2O_APT_PAUSE:-20}
# A mirror that has stopped answering is left after 30 seconds, and a file that did not arrive is asked for
# again, by apt-get itself. Another apt or dpkg that holds the lock (the runner's own, shortly after it
# started) is waited for.
OPTIONS="-o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30 -o DPkg::Lock::Timeout=120"
# The runner's user is not root; a container's is, and need not have sudo.
SUDO=sudo
[ "$(id -u)" != 0 ] || SUDO=

# Run apt-get with a time limit, up to three times. timeout is started by sudo, not the other way round:
# it then stops apt-get itself, as root, and not a sudo that has to pass the signal on.
attempt() { # <seconds for one try> <what it is, for the messages> <apt-get arguments...>
    limit=$1 what=$2
    shift 2
    try=1
    while :; do
        code=0
        # shellcheck disable=SC2086  # a list of options
        $SUDO timeout --kill-after=10 "$limit" apt-get $OPTIONS "$@" || code=$?
        [ "$code" != 0 ] || return 0
        case $code in
            124 | 137) why="was stopped after $limit seconds" ;;
            *) why="failed (exit $code)" ;;
        esac
        echo "apt-install: $what $why, try $try of $TRIES" >&2
        [ "$try" -lt "$TRIES" ] || return 1
        try=$((try + 1))
        sleep "$PAUSE"
        [ -z "$AGAIN" ] || lists_again
    done
}
# The package lists once more, before a download is tried again. apt-get update ends well when a mirror
# could not be reached ("Some index files failed to download. They have been ignored, or old ones used
# instead." is a warning then, and an error only when the mirror answers with one). The lists that are
# there, from such an update or from the runner's image, can name a file the mirror has replaced since:
# the download asked for it three times, got 404 three times and failed, where apt-get's own advice is to
# run update. Seen with apt 2.7.14 against a mirror that dropped its connections for 15 seconds and came
# back with a newer package. What this fetch cannot get stays as it is; the try that follows shows
# whether that matters.
lists_again() {
    # shellcheck disable=SC2086  # a list of options
    $SUDO timeout --kill-after=10 60 apt-get $OPTIONS update ||
        echo "apt-install: the package lists could not be fetched again; the next try uses the ones that are there" >&2
}

AGAIN=
if [ -n "$UPDATE" ]; then
    attempt 60 "fetching the package lists" update ||
        { echo "apt-install: the package lists could not be fetched; nothing was installed" >&2; exit 1; }
fi
AGAIN=1
attempt 300 "downloading the packages" install -y --no-install-recommends --download-only "$@" ||
    { echo "apt-install: the packages could not be downloaded; nothing was installed" >&2; exit 1; }
# shellcheck disable=SC2086  # a list of options
$SUDO apt-get $OPTIONS install -y --no-install-recommends "$@"
