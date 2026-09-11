"""Reticulum (rnsd) daemon loop for the Chaquopy Python runtime.

Runs the reference RNS implementation inside the app process (no separate
binary needed on Android — RNS is pure Python with optional deps). The
foreground service keeps this thread alive while the app is backgrounded.

Config dir receives RNS's auto-generated `config` + identity files on first
start, mirroring rnsd's behavior on desktop. RNS log output (RNS.log) is
routed to <configdir>/logfile, which the Rust bridge surfaces for debug.
"""

import os
import signal
import sys
import time

# Reticulum's Reticulum.__init__ unconditionally registers signal handlers:
#   signal.signal(signal.SIGINT, Reticulum.sigint_handler)
#   signal.signal(signal.SIGTERM, Reticulum.sigterm_handler)
# In Android (Chaquopy), the daemon runs on a background worker thread.
# Calling signal.signal() on any thread other than Python's main thread
# raises ValueError: signal only works in main thread of the main interpreter.
# Catch and ignore ValueError so Reticulum starts cleanly in-process.
try:
    _orig_signal = signal.signal

    def _safe_signal(sig, handler):
        try:
            return _orig_signal(sig, handler)
        except (ValueError, AttributeError):
            return None

    signal.signal = _safe_signal
except Exception:
    pass

_running = False
_start_error = None
_started_at = None
_reticulum = None
_configdir = None


def start(configdir):
    """Start the Reticulum instance. Returns True on success, False on any
    initialization failure (RNS import error, config/identity write error,
    interface bring-up error, ...). Startup happens on the calling TThread so
    a blocked RNS bring-up never stalls the app."""
    global _running, _start_error, _started_at, _reticulum, _configdir
    if _running:
        return True
    _configdir = configdir
    try:
        import RNS

        inst = None
        try:
            inst = RNS.Reticulum.get_instance()
        except Exception:
            inst = getattr(RNS.Reticulum, "_Reticulum__instance", None)
        if inst is not None:
            _reticulum = inst
            _running = True
            _start_error = None
            _started_at = _started_at or time.time()
            RNS.log("rnsd resumed (Chaquopy, RNS %s)" % RNS.version(), RNS.LOG_NOTICE)
            return True

        RNS.log("rnsd starting (Chaquopy, configdir=%s)" % configdir, RNS.LOG_NOTICE)
        # logdest=RNS.LOG_FILE makes RNS write to <configdir>/logfile
        # (Reticulum.__init__ sets RNS.logfile when the dest is LOG_FILE).
        _reticulum = RNS.Reticulum(
            configdir=configdir,
            logdest=RNS.LOG_FILE,
            loglevel=RNS.LOG_NOTICE,
        )
        _running = True
        _start_error = None
        _started_at = time.time()
        RNS.log("rnsd started (Chaquopy, RNS %s)" % RNS.version(), RNS.LOG_NOTICE)
        return True
    except Exception as exc:  # noqa: BLE001 - report any init failure
        sys.stderr.write("rnsd start failed: %r\n" % (exc,))
        _running = False
        _reticulum = None
        _start_error = repr(exc)
        _started_at = None
        return False


def stop():
    """Stop the daemon loop. RNS has no in-process teardown for its background
    transport threads, so the loop just exits; the thread is a daemon and the
    instance is dropped (its app-facing handle released next start)."""
    global _running, _reticulum
    _running = False
    _reticulum = None
    return True


def is_running():
    return _running


def status():
    """JSON-ish dict for the Rust side: liveness, startup failure, uptime."""
    return {
        "running": _running,
        "error": _start_error,
        "started_at": _started_at,
        "configdir": _configdir,
    }


def _pump():
    """Keep the daemon thread alive. RNS.Reticulum manages its own worker
    threads; the sleep loop just holds the thread (mirrors the rnsd CLI main
    loop) so the reference implementation stays resident in-process."""
    while _running:
        time.sleep(1)