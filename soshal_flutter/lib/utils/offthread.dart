/// Off-thread decode helper.
///
/// Runs decoding tasks off the UI thread via [Isolate.run]. If a closure captures
/// unsendable lexical scope (e.g. [ChangeNotifier] or framework objects), it safely
/// falls back to execution in the calling isolate rather than throwing an error.
library;

import 'dart:async';
import 'dart:io';
import 'dart:isolate';

bool get _isTest =>
    Platform.environment['FLUTTER_TEST'] == 'true' ||
    const bool.fromEnvironment('FLUTTER_TEST');

/// Call [fn] off the main UI isolate when possible, falling back to in-isolate
/// execution if the closure captures unsendable state.
///
/// Unlike [runOffThreadCompute] this cannot apply a size threshold: the payload
/// is captured in a closure rather than passed, so there is nothing to measure
/// without inspecting the closure's captured scope. Callers that want the fast
/// path pass their payload explicitly.
Future<T> runOffThread<T>(FutureOr<T> Function() fn) async {
  if (_isTest) {
    return await fn();
  }
  try {
    return await Isolate.run(fn);
  } catch (_) {
    // Fallback: execute in the calling isolate if isolate transmission fails.
    return await fn();
  }
}

/// Typed helper that passes [payload] explicitly to [parser] in an isolate,
/// guaranteeing that no lexical instance scope is captured.
///
/// Small payloads skip the isolate entirely — see [shouldParseInline].
Future<R> runOffThreadCompute<T, R>(R Function(T) parser, T payload) async {
  if (_isTest || shouldParseInline(payload)) {
    return parser(payload);
  }
  try {
    return await Isolate.run(() => parser(payload));
  } catch (_) {
    return parser(payload);
  }
}

/// Payloads below this many units are parsed on the calling isolate.
///
/// [Isolate.run] allocates an isolate and a message port per call, which costs
/// 1-3 ms before the closure body runs at all. Decoding a few kilobytes of JSON
/// is well under 0.1 ms, so below this size the spawn dominates the work it
/// would parallelize by an order of magnitude. The cutoff is deliberately low:
/// at 8 KiB the parse is still far cheaper than the spawn, and every caller
/// that hands this a JSON string gets the fast path with no call-site edits.
const int kInlineParseBytes = 8 * 1024;

/// A cheap lower bound on [payload]'s size, or null when it cannot be had
/// without walking the payload.
///
/// For a [String] this is UTF-16 code units, which for the JSON carried by these
/// callers is a conservative under-estimate of the byte count — the safe
/// direction for a threshold, since it can only under-size, never over-size.
/// A [List]'s length is likewise a lower bound: each element is at least one
/// unit of work. Anything else returns null, which means "unknown" and keeps the
/// off-thread behavior.
int? estimatePayloadSize(Object? payload) => switch (payload) {
      final String s => s.length,
      final List<Object?> l => l.length,
      _ => null,
    };

/// Whether [payload] is small enough that spawning an isolate cannot pay for
/// itself.
///
/// This is the entire behavioral surface of the threshold: the parsers passed
/// to [runOffThreadCompute] are top-level pure functions, so running one on the
/// calling isolate instead of a fresh one cannot change the value it returns.
/// Only the timing differs, which is the whole point.
bool shouldParseInline(Object? payload) {
  final size = estimatePayloadSize(payload);
  return size != null && size < kInlineParseBytes;
}
