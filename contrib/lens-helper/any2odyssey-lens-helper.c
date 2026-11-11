/* SPDX-License-Identifier: MIT
 *
 * any2odyssey-lens-helper: keeps the Odyssey 3D's lens switched on while it
 * runs, by asking Leia's SR runtime through the srSDK C API.
 *
 * This file is NOT part of the GPL-3.0-only Any2Odyssey program. It is a
 * separate program under the MIT license so that it may be linked with Leia's
 * commercially licensed SDK loader. Any2Odyssey starts it and talks to it
 * only through its standard streams (docs/LENS-SWITCH.md):
 *
 *   started            -> asks the runtime to switch the lens on
 *   prints "on"        -> the runtime accepted the request
 *   prints "error: …"  -> it could not; exits 1
 *   standard input closes, or SIGTERM/SIGINT/SIGHUP
 *                      -> withdraws the request and exits 0
 *
 * The runtime keeps the lens on only while a connected client asks for it, so
 * the request lives exactly as long as this process.
 *
 * Written from the public description of the srSDK 1.0.0 API in the
 * DisplayXR Leia plug-in (Apache-2.0); it has not yet been compiled against
 * the SDK or run on a display.
 */
/* sigaction and nanosleep under -std=c99. */
#define _POSIX_C_SOURCE 200809L

#include <sr/sr.h>

#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>

/* How long to wait for the SR service to answer before giving up. */
#define CONNECT_BUDGET_MS 5000
#define CONNECT_STEP_MS 250

static volatile sig_atomic_t g_stop = 0;

static void on_signal(int sig)
{
	(void)sig;
	g_stop = 1;
}

static void say(const char *line)
{
	puts(line);
	fflush(stdout);
}

static int fail(const char *what, SrResult res)
{
	printf("error: %s: %s\n", what, srResultToString(res));
	fflush(stdout);
	return 1;
}

int main(void)
{
	/* No SA_RESTART: a signal must interrupt the read below. */
	struct sigaction sa;
	sa.sa_handler = on_signal;
	sa.sa_flags = 0;
	sigemptyset(&sa.sa_mask);
	sigaction(SIGTERM, &sa, NULL);
	sigaction(SIGINT, &sa, NULL);
	sigaction(SIGHUP, &sa, NULL);
	/* The parent going away must not kill us before the lens is released. */
	signal(SIGPIPE, SIG_IGN);

	SrInstance instance;
	SrResult res;
	for (int waited_ms = 0;; waited_ms += CONNECT_STEP_MS) {
		SrInstanceCreateInfo ci = SrInstanceCreateInfo(.applicationName = "Any2Odyssey");
		res = srCreateInstance(&ci, &instance);
		if (SR_SUCCEEDED(res)) {
			break;
		}
		if (res != SR_ERROR_RUNTIME_UNAVAILABLE || waited_ms >= CONNECT_BUDGET_MS || g_stop) {
			if (res == SR_ERROR_RUNTIME_UNAVAILABLE) {
				printf("error: the Leia SR runtime is not available (%s; loader: %s)\n",
				       srResultToString(res), srGetLastLoaderError());
				fflush(stdout);
				return 1;
			}
			return fail("srCreateInstance", res);
		}
		struct timespec step = {0, CONNECT_STEP_MS * 1000000L};
		nanosleep(&step, NULL);
	}

	res = srInitialize(instance);
	if (SR_FAILED(res)) {
		int code = fail("srInitialize", res);
		srDestroyInstance(instance);
		return code;
	}

	SrLens lens = NULL;
	SrLensCreateInfo lci = SrLensCreateInfo();
	res = srCreateLens(instance, &lci, &lens);
	if (SR_FAILED(res)) {
		int code = fail("this display has no switchable lens the runtime can reach (srCreateLens)", res);
		srDestroyInstance(instance);
		return code;
	}

	res = srLensEnable(lens);
	if (SR_FAILED(res)) {
		int code = fail("srLensEnable", res);
		srDestroyLens(lens);
		srDestroyInstance(instance);
		return code;
	}
	say("on");

	/* Hold the request until the input closes or a signal arrives. */
	char discard[64];
	while (!g_stop) {
		ssize_t n = read(STDIN_FILENO, discard, sizeof(discard));
		if (n == 0 || (n < 0 && errno != EINTR)) {
			break;
		}
	}

	srLensDisable(lens);
	srDestroyLens(lens);
	srDestroyInstance(instance);
	return 0;
}
