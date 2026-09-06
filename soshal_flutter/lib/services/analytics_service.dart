// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Analytics Service
/// Local SLM statistics, embeddings and post classification.
class AnalyticsService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  /// Compute engagement/posts stats. Backend stub: returns "stats".
  Future<String> computeStats() => guard(() {
        return RustLib.instance.api.crateFfiAnalyticsAnalyticsComputeStats();
      });

  /// Generate a dense vector embedding for text. Throws on empty input.
  Future<String> generateEmbedding(String text) => guard(() {
        return RustLib.instance.api
            .crateFfiAnalyticsAnalyticsSlmGenerateEmbedding(
          text: text,
        );
      });

  /// Classify post text (sentiment + spam) via local SLM.
  Future<String> classifyPost(String text) => guard(() {
        return RustLib.instance.api.crateFfiAnalyticsAnalyticsSlmClassifyPost(
          text: text,
        );
      });
}
