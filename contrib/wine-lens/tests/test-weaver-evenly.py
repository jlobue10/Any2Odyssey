#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The weaver host's rule for a film's pictures (`evenly_hold` in src/weaver-host.c), without Wine or a monitor.

The rule is cut out of the source file as it stands and compiled with the system's C compiler beside the few
lines of the host's loop that call it. Picture times are put through it on a clock of the test's own: a turn
of the loop is one refresh of a display, and looks at the newest picture written by then.

What is checked: a film whose pictures come a whole number of refreshes apart, some of them late, is shown
for as many refreshes each; no picture is left until the next one has taken its place (it would never be
shown); pictures that come at no such rate are shown at the turn that finds them; the first pictures, a
pause and the turn counter's wrap.
"""
import math
import random
import subprocess
import tempfile
import unittest
from pathlib import Path

SOURCE = Path(__file__).resolve().parent.parent / "src" / "weaver-host.c"

HARNESS = r"""
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef int LONG; /* 32 bits, as on Windows */
#include "rule.h"
/* input: millihertz, the look's place in the refresh (ns), turns, whether the rule is followed, the turn
 * counter at the start, the count of pictures, then the time each picture is whole in the file (ns, rising).
 * output: for each picture the turn that took it (-1: none did). */
int main(void)
{
    long long millihertz, phase, turns, start, count, *written;
    int even;
    struct evenly evenly = { 0 };
    LONG shown = 0;
    long long newest = 0;
    if (scanf("%lld %lld %lld %d %lld %lld", &millihertz, &phase, &turns, &even, &start, &count) != 6) return 2;
    written = malloc((count + 1) * sizeof *written);
    for (long long i = 0; i < count; i++) if (scanf("%lld", &written[i]) != 1) return 2;
    long long *taken = malloc((count + 1) * sizeof *taken);
    for (long long i = 0; i < count; i++) taken[i] = -1;
    evenly.turn = evenly.shown_at = (unsigned)start;
    for (long long k = 0; k < turns; k++) {
        long long look = phase + k * 1000000000000LL / millihertz;
        LONG sequence;
        bool early;
        while (newest < count && written[newest] <= look) newest++;
        sequence = (LONG)(2 * newest);
        /* as main() has it */
        evenly.turn++;
        early = sequence != shown && !(sequence & 1) && evenly_hold(&evenly, sequence);
        if (early && even) {
            /* left for the next turn */
        } else if (sequence != shown && !(sequence & 1)) {
            shown = sequence;
            evenly.shown_at = evenly.turn;
            taken[newest - 1] = k;
        }
    }
    for (long long i = 0; i < count; i++) printf("%lld\n", taken[i]);
    return 0;
}
"""

SECOND = 1_000_000_000
MS = 1_000_000


def the_rule():
    """`struct evenly` and `evenly_hold`, as the source file has them."""
    lines = SOURCE.read_text(encoding="utf-8").split("\n")
    start = next(i for i, line in enumerate(lines) if line.startswith("#define EVEN_RUN"))
    head = next(i for i, line in enumerate(lines) if line.startswith("static bool evenly_hold("))
    end = next(i for i in range(head, len(lines)) if lines[i] == "}")
    assert start < head < end
    return "\n".join(lines[start:end + 1]) + "\n"


class Shown:
    """What became of each picture of one run."""

    def __init__(self, written, taken, hertz, phase):
        self.written, self.taken = written, taken
        period = SECOND / hertz
        # The first turn that could have taken each picture.
        self.found = [max(0, math.ceil((at - phase) / period - 1e-9)) for at in written]

    def never(self):
        """Pictures no turn took, the ones after the last turn's aside."""
        last = max((i for i, turn in enumerate(self.taken) if turn >= 0), default=-1)
        return [i + 1 for i in range(last) if self.taken[i] < 0]

    def turns_each(self):
        """For how many turns each picture was shown (but the last)."""
        turns = [turn for turn in self.taken if turn >= 0]
        return [later - sooner for sooner, later in zip(turns, turns[1:])]

    def left_for(self):
        """For how many turns each picture that was shown had been left."""
        return [turn - found for turn, found in zip(self.taken, self.found) if turn >= 0]


class EvenlyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.built = tempfile.TemporaryDirectory(prefix="a2o-weaver-evenly-test-")
        cls.addClassCleanup(cls.built.cleanup)
        folder = Path(cls.built.name)
        (folder / "rule.h").write_text(the_rule())
        (folder / "harness.c").write_text(HARNESS)
        cls.program = str(folder / "harness")
        subprocess.run(["cc", "-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror", "-fsanitize=address,undefined",
                        "-o", cls.program, str(folder / "harness.c")], check=True)

    def shown(self, written, hertz, even=True, phase=0.5, start=0, seconds=None):
        phase = round(phase * SECOND / hertz)
        seconds = seconds if seconds is not None else written[-1] / SECOND + 1
        text = " ".join(str(number) for number in (round(hertz * 1000), phase, round(seconds * hertz), int(even),
                                                   start, len(written), *written))
        result = subprocess.run([self.program], input=text, capture_output=True, text=True, timeout=120,
                                env={"ASAN_OPTIONS": "detect_leaks=0"})
        self.assertEqual(result.returncode, 0, result.stderr)
        return Shown(written, [int(line) for line in result.stdout.split()], hertz, phase)

    # Sources.

    @staticmethod
    def even_source(rate, seconds, first=MS // 2):
        return [first + round(i * SECOND / rate) for i in range(int(seconds * rate))]

    @staticmethod
    def film(rate, hertz, seconds, seed=1):
        """Pictures as they were measured on the Odyssey at 120 Hz: made at a refresh of the display, three
        of four written within 2 ms of one point of it and the rest up to 6 ms after that point, in
        stretches. (At another refresh rate the same parts of a refresh.)"""
        draw = random.Random(seed)
        MS = round(1_000_000 * 120 / hertz)
        late_ms = 6.0
        written, late, last = [], False, -1
        for i in range(int(seconds * rate)):
            refresh = math.ceil(i * hertz / rate - 1e-9)
            if refresh == last:
                continue
            last = refresh
            if draw.random() < (1 / 8 if late else 1 / 24):
                late = not late
            after = 2 * MS + draw.random() * (late_ms - 2) * MS if late else draw.random() * 2 * MS
            at = round(refresh * SECOND / hertz + after)
            written.append(max(at, written[-1] + 1000) if written else at)
        return written

    @staticmethod
    def wandering(rate, hertz, seconds, seed=2):
        """A game near `rate`: its pictures reach the desktop at refreshes, two a second up or down."""
        draw = random.Random(seed)
        written, now, at, last = [], rate, 0.0, -1
        while at < seconds:
            now = min(rate + 2, max(rate - 2, now + (draw.random() - 0.5) * 0.4))
            at += 1 / now
            refresh = math.ceil(at * hertz)
            if refresh != last:
                written.append(round(refresh * SECOND / hertz + draw.random() * 2 * MS))
            last = refresh
        return written

    # Tests.

    def test_a_film_is_shown_for_as_many_refreshes_each(self):
        # 24 a second at 120 Hz, a minute, at each place of the turn within the refresh.
        for phase in (0.1, 0.3, 0.5, 0.7, 0.9):
            written = self.film(24, 120, 60)
            uneven = lambda shown: sum(1 for turns in shown.turns_each()[40:] if turns != 5)
            with_rule, without = self.shown(written, 120, phase=phase), self.shown(written, 120, even=False, phase=phase)
            self.assertEqual(with_rule.never(), [])
            self.assertLessEqual(uneven(with_rule), 4, f"phase {phase}: {uneven(without)} without the rule")
            # No picture waits for more than three turns, and without the rule none waits at all.
            self.assertLessEqual(max(with_rule.left_for()), 3)
            self.assertEqual(max(without.left_for()), 0)
        # At one of those places the late pictures miss the turn: that is what the rule is for.
        written = self.film(24, 120, 60)
        worst = max(sum(1 for turns in self.shown(written, 120, even=False, phase=phase).turns_each() if turns != 5)
                    for phase in (0.1, 0.3, 0.5, 0.7, 0.9))
        self.assertGreater(worst, 100)

    def test_no_picture_is_left_until_the_next_has_taken_its_place(self):
        # A picture that is left is still the newest only until the next is written. Each of these sources
        # had pictures that were left for so long that they were never shown: one in 25 of the first.
        sources = {
            "50 a second at 144 Hz (2.88 refreshes apart, which pass for 3)": (self.even_source(50, 60), 144),
            "40 a second at 120 Hz from a clock that is a little fast": (self.even_source(40 * 1.002, 60), 120),
            "48 a second at 144 Hz from a clock that is a little fast": (self.even_source(48 * 1.002, 60), 144),
            "30 a second at 120 Hz from a clock that is a little fast": (self.even_source(30 * 1.002, 60), 120),
            "a game near 30 a second at 120 Hz": (self.wandering(30, 120, 120), 120),
            "a game near 40 a second at 120 Hz": (self.wandering(40, 120, 120), 120),
            "a film of 24 at 120 Hz": (self.film(24, 120, 60), 120),
            "a film of 30 at 120 Hz": (self.film(30, 120, 60), 120),
            "a video of 40 at 120 Hz": (self.film(40, 120, 60), 120),
            "a video of 20 at 60 Hz": (self.film(20, 60, 60), 60),
            "a video of 60 at 240 Hz": (self.film(60, 240, 60), 240),
        }
        for name, (written, hertz) in sources.items():
            with self.subTest(name):
                self.assertEqual(self.shown(written, hertz, even=False).never(), [], "without the rule")
                self.assertEqual(self.shown(written, hertz).never(), [])

    def test_a_picture_is_never_left_into_the_turn_that_may_find_the_next(self):
        # Three turns at the most, and fewer where the pictures are closer together than five turns: the next
        # comes that many turns later, give or take one.
        for rate, hertz, most in ((24, 120, 3), (20, 120, 3), (30, 120, 2), (40, 120, 1), (20, 60, 1), (60, 240, 2)):
            with self.subTest(f"{rate} at {hertz}"):
                left = self.shown(self.film(rate, hertz, 60), hertz).left_for()
                self.assertLessEqual(max(left), most)
                self.assertGreater(max(left), 0, "the rule was never followed")
                # A source whose clock runs ahead of the display's is found a little sooner each time, until
                # its pictures are left for as long as they may be.
                left = self.shown(self.even_source(rate * 1.002, 60), hertz).left_for()
                self.assertEqual(max(left), most)

    def test_pictures_at_no_such_rate_are_shown_as_they_are_found(self):
        # Not a whole number of refreshes apart, or fewer than three: the rule changes nothing.
        for rate, hertz in ((25, 120), (50, 120), (60, 120), (120, 120), (30, 100), (30, 144), (60, 144), (48, 165),
                            (60, 165), (30, 60), (24, 60), (60, 60)):
            with self.subTest(f"{rate} at {hertz}"):
                for written in (self.film(rate, hertz, 30), self.even_source(rate, 30)):
                    self.assertEqual(self.shown(written, hertz).taken, self.shown(written, hertz, even=False).taken)
        # Nor does it for a desktop, whose pictures come when something changes.
        draw = random.Random(3)
        written, at = [], 0
        for _ in range(3000):
            at += round((1 + int(draw.random() * 12)) * SECOND / 120 + draw.random() * 2 * MS)
            written.append(at)
        self.assertEqual(self.shown(written, 120).taken, self.shown(written, 120, even=False).taken)

    def test_the_first_pictures_and_the_ones_after_a_pause_are_shown_as_they_are_found(self):
        # Every fourth picture misses the turn that takes the others (written 6 ms after its refresh, where
        # the turn looks 4 ms after it). Without the rule each of those is shown for four turns and the one
        # before it for six.
        written = [round(i * 5 * SECOND / 120) + (6 * MS if i % 4 == 2 else MS) for i in range(480)]
        with_rule, without = self.shown(written, 120), self.shown(written, 120, even=False)
        self.assertEqual(sorted(set(without.turns_each())), [4, 5, 6])
        # The rule needs a run of 32 pictures: the 32nd is the first to be left, for the one turn its late
        # neighbour was behind, and from there on every picture is shown for five turns.
        self.assertEqual(with_rule.taken[:31], without.taken[:31])
        self.assertEqual(with_rule.left_for()[:31], [0] * 31)
        self.assertEqual(with_rule.left_for()[31], 1)
        self.assertEqual(set(with_rule.turns_each()[31:]), {5})
        # A pause of two seconds in the film: the picture after it and the 30 that follow are no run yet.
        paused = written[:240] + [at + 2 * SECOND for at in written[240:]]
        with_rule = self.shown(paused, 120)
        self.assertEqual(with_rule.never(), [])
        self.assertEqual(with_rule.left_for()[240:271], [0] * 31)
        self.assertEqual(with_rule.left_for()[271], 1)

    def test_a_picture_that_is_late_by_more_than_a_turn_does_not_end_the_run(self):
        # One picture in 40 comes 20 ms late (two and a half refreshes): it is shown when it comes, and the
        # others go on being shown for five turns each. (All but two of a run have to be within a turn.)
        written = self.film(24, 120, 60)
        for i in range(45, len(written), 40):
            written[i] += 20 * MS
        uneven = lambda shown: sum(1 for turns in shown.turns_each()[40:] if turns != 5)
        with_rule, without = self.shown(written, 120), self.shown(written, 120, even=False)
        self.assertEqual(with_rule.never(), [])
        self.assertGreater(uneven(without), 250)
        self.assertLess(uneven(with_rule), 100)

    def test_the_turn_counter_may_wrap(self):
        written = self.film(24, 120, 60)
        self.assertEqual(self.shown(written, 120, start=2**32 - 3000).taken, self.shown(written, 120).taken)


if __name__ == "__main__":
    unittest.main()
