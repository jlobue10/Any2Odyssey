// SPDX-License-Identifier: MIT
// KWin script, loaded by any2odyssey-weaver while it shows a live screen (KDE Plasma 6).
// The 3D picture covers the Odyssey; when the screen being shown is the Odyssey's own, the picture must not
// be part of what is captured, or it would show a picture of itself. KWin can leave a window out of screen
// casts; this asks it to for the weaver host's window, and keeps that window above the others and out of
// the task bar and the window switcher.
function arrange(window) {
    if (window && window.resourceClass == "any2odyssey-weaver.exe" && !window.excludeFromCapture) {
        window.excludeFromCapture = true;
        window.keepAbove = true;
        window.skipTaskbar = true;
        window.skipSwitcher = true;
        window.skipPager = true;
    }
}
function arrangeAll() {
    workspace.windowList().forEach(arrange);
}
// A window of a Wine program can be announced before it has its class, and is then not recognized by
// arrange(). So the list is looked over again twice a second for ten seconds after this script is loaded and
// after every window that appears, and then left alone: looking for as long as the script is loaded kept the
// desktop busy for a window that had been found long before.
var LOOKS = 20;
var looksLeft = 0;
var again = new QTimer();
again.interval = 500;
again.timeout.connect(function () {
    arrangeAll();
    looksLeft -= 1;
    if (looksLeft <= 0) {
        again.stop();
    }
});
function lookForAWhile() {
    looksLeft = LOOKS;
    again.start();
}
arrangeAll();
workspace.windowAdded.connect(function (window) {
    arrange(window);
    lookForAWhile();
});
lookForAWhile();
