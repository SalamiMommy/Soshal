// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Analytics Service
/// Local SLM statistics, embeddings and post classification.
class AnalyticsService extends ChangeNotifier {
  String? _lastError;

  String? get lastError => _lastError;

  /// Compute engagement/posts stats. Backend stub: returns "stats".
  Future<String> computeStats() async {
    try {
      final out = RustLib.instance.api.crateFfiAnalyticsAnalyticsComputeStats();
      _lastError = null;
      notifyListeners();
      return out;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Generate a dense vector embedding for text. Throws on empty input.
  Future<String> generateEmbedding(String text) async {
    try {
      final out =
          RustLib.instance.api.crateFfiAnalyticsAnalyticsSlmGenerateEmbedding(
        text: text,
      );
      _lastError = null;
      notifyListeners();
      return out;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Classify post text (sentiment + spam) via local SLM.
  Future<String> classifyPost(String text) async {
    try {
      final out =
          RustLib.instance.api.crateFfiAnalyticsAnalyticsSlmClassifyPost(
        text: text,
      );
      _lastError = null;
      notifyListeners();
      return out;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }
}
