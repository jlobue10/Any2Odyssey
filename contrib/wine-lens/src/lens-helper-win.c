/* SPDX-License-Identifier: MIT
 *
 * any2odyssey-lens-helper, Windows SR runtime flavour (runs on Windows or under Wine).
 *
 * Asks the Leia SR runtime to switch the display's lens on and keeps asking for as long as standard input
 * stays open. Same contract as the Linux helper: prints "on" once the request is in, or "error: <reason>";
 * on end of input it withdraws the request and exits.
 *
 * It uses a handful of functions of the runtime's public C API (sr/core_c.h, sr/displays_c.h in Leia's SDK),
 * looked up by name in the installed runtime, so neither the SDK nor any file of Leia's is needed to build it.
 */
#include <stdbool.h>
#include <stdio.h>
#include <windows.h>

typedef void *SRContext;
typedef void *SR_switchableLensHint;

static SRContext (*newSRContext)(void);
static void (*initializeSRContext)(SRContext);
static void (*deleteSRContext)(SRContext);
static SR_switchableLensHint (*createSwitchableLensHint)(SRContext);
static void (*lensEnableHint)(SR_switchableLensHint);
static void (*lensDisableHint)(SR_switchableLensHint);
static bool (*isLensEnabled)(SR_switchableLensHint);
static void (*deleteSwitchableLensHint)(SR_switchableLensHint);

static bool find_runtime(void)
{
    HMODULE core = LoadLibraryA("SimulatedRealityCore.dll");
    HMODULE displays = LoadLibraryA("SimulatedRealityDisplays.dll");
    if (!core || !displays) return false;
#define GET(module, name) (name = (void *)GetProcAddress(module, #name))
    return GET(core, newSRContext) && GET(core, initializeSRContext) && GET(core, deleteSRContext) &&
           GET(displays, createSwitchableLensHint) && GET(displays, lensEnableHint) && GET(displays, lensDisableHint) &&
           GET(displays, isLensEnabled) && GET(displays, deleteSwitchableLensHint);
#undef GET
}

int main(void)
{
    SRContext context;
    SR_switchableLensHint lens;
    char buf[64];
    int tries;

    setvbuf(stdout, NULL, _IONBF, 0);
    if (!find_runtime()) {
        puts("error: the Leia SR runtime libraries were not found");
        return 1;
    }
    context = newSRContext();
    if (!context) {
        puts("error: could not reach the SR service");
        return 1;
    }
    lens = createSwitchableLensHint(context);
    if (!lens) {
        puts("error: this display has no switchable lens");
        deleteSRContext(context);
        return 1;
    }
    initializeSRContext(context);
    lensEnableHint(lens);
    puts("on");

    /* Report what the runtime says for a moment; purely informational. */
    for (tries = 0; tries < 20 && !isLensEnabled(lens); tries++) Sleep(100);
    printf("status: lens %s\n", isLensEnabled(lens) ? "enabled" : "not enabled yet");

    while (fgets(buf, sizeof buf, stdin)) {
    }

    lensDisableHint(lens);
    Sleep(300); /* let the request reach the service before the connection drops */
    deleteSwitchableLensHint(lens);
    deleteSRContext(context);
    return 0;
}
