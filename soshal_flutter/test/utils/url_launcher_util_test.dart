import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/utils/url_launcher_util.dart';

void main() {
  group('UrlLauncherUtil', () {
    test('launchSafeUrl rejects empty or invalid schemes', () async {
      expect(await UrlLauncherUtil.launchSafeUrl(null, ''), isFalse);
      expect(await UrlLauncherUtil.launchSafeUrl(null, '   '), isFalse);
      expect(await UrlLauncherUtil.launchSafeUrl(null, 'javascript:alert(1)'), isFalse);
      expect(await UrlLauncherUtil.launchSafeUrl(null, 'file:///etc/passwd'), isFalse);
      expect(await UrlLauncherUtil.launchSafeUrl(null, 'ftp://files.example.com'), isFalse);
    });
  });
}
