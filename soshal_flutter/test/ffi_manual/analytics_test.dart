// Manual ffi tests for analytics
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/analytics.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-analytics-manual');
  final api = env.$1;

  test('analyticsComputeStats calls crateFfiAnalyticsAnalyticsComputeStats', () {
    api.stubString('crateFfiAnalyticsAnalyticsComputeStats', 'ok');
    final res = analyticsComputeStats();
    expect(res, 'ok');
    expect(api.callCount('crateFfiAnalyticsAnalyticsComputeStats'), 1);
  });

  test('analyticsSlmGenerateEmbedding calls crateFfiAnalyticsAnalyticsSlmGenerateEmbedding', () {
    api.stubString('crateFfiAnalyticsAnalyticsSlmGenerateEmbedding', '[0.1,0.2]');
    final res = analyticsSlmGenerateEmbedding(text: 'hi');
    expect(res, '[0.1,0.2]');
    expect(api.callCount('crateFfiAnalyticsAnalyticsSlmGenerateEmbedding'), 1);
  });

  test('analyticsSlmClassifyPost calls crateFfiAnalyticsAnalyticsSlmClassifyPost', () {
    api.stubString('crateFfiAnalyticsAnalyticsSlmClassifyPost', 'spam');
    final res = analyticsSlmClassifyPost(text: 'hi');
    expect(res, 'spam');
    expect(api.callCount('crateFfiAnalyticsAnalyticsSlmClassifyPost'), 1);
  });
}
