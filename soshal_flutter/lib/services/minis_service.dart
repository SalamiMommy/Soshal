// ignore_for_file: invalid_use_of_internal_member
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Minis: mini-app registry (URLs) plus WASI content-filter / feed-ranker
/// plugin execution. Stateless wrapper — screens own their UI state.
class MinisService with LastErrorMixin {
  bool _wasmRuntimeUnavailable = false;

  /// True after a plugin call fails: the WASI host is on the roadmap, so
  /// execution is simulated and errors are expected.
  bool get wasmRuntimeUnavailable => _wasmRuntimeUnavailable;

  /// Fetch known mini URLs. Backend stub returns an empty list today.
  List<String> fetchMinis() {
    try {
      final minis = RustLib.instance.api.crateFfiMinisMinisFetch();
      clearLastError();
      return minis;
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }

  /// Run a WASI content-filter plugin against text.
  String runFilter({
    required String pluginId,
    required String text,
    required String wasmBytesHex,
  }) {
    try {
      final result = RustLib.instance.api.crateFfiMinisMinisWasmExecuteFilter(
        pluginId: pluginId,
        text: text,
        wasmBytesHex: wasmBytesHex,
      );
      clearLastError();
      _wasmRuntimeUnavailable = false;
      return result;
    } catch (e, st) {
      setLastError(e, st);
      _wasmRuntimeUnavailable = true;
      return '';
    }
  }

  /// Run a WASI feed-ranker plugin over candidate posts (JSON strings).
  List<String> rankFeed({
    required String pluginId,
    required List<String> postsJson,
    required String wasmBytesHex,
  }) {
    try {
      final ranked = RustLib.instance.api.crateFfiMinisMinisWasmRankFeed(
        pluginId: pluginId,
        postsJson: postsJson,
        wasmBytesHex: wasmBytesHex,
      );
      clearLastError();
      _wasmRuntimeUnavailable = false;
      return ranked;
    } catch (e, st) {
      setLastError(e, st);
      _wasmRuntimeUnavailable = true;
      return const [];
    }
  }
}
