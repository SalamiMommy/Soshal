"""Reticulum (rnsd) daemon loop for the Chaquopy Python runtime.

Runs the reference RNS implementation inside the app process (no separate
binary needed on Android — RNS is pure Python with optional deps). The
foreground service keeps this thread alive while the app is backgrounded.

Config dir receives RNS's auto-generated `config` + identity files on first
start, mirroring rnsd's behavior on desktop.
"""

import sys
import time

_running = False
_reticulum = None


def start(configdir):
    global _running, _reticulum
    if _running:
        return True
    try:
        import RNS

        _running = True
        _reticulum = RNS.Reticulum(
            configdir=configdir,
            logdest=RNS.LOG_FILE,
        )
        RNS.log(
            "rnsd started (Chaquopy, configdir=%s)" % configdir,
            RNS.LOG_NOTICE,
        )
        return True
    except Exception as exc:  # noqa: BLE001 - report any init failure
        sys.stderr.write("rnsd start failed: %r\n" % (exc,))
        _running = False
        _reticulum = None
        return False


def stop():
    global _running
    _running = False
    return True


def is_running():
    return _running


def _pump():
    while _running:
        time.sleep(1)
