import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/utils/safe_url.dart';

void main() {
  group('SafeUrl.isSafeMediaUrl', () {
    test('accepts public hosts', () {
      expect(SafeUrl.isSafeMediaUrl('https://example.com/img.png'), isTrue);
      expect(SafeUrl.isSafeMediaUrl('http://cdn.example.net/a'), isTrue);
      expect(SafeUrl.isSafeMediaUrl('  https://example.com/x  '), isTrue);
    });

    test('rejects non-http schemes and junk', () {
      expect(SafeUrl.isSafeMediaUrl(''), isFalse);
      expect(SafeUrl.isSafeMediaUrl('ftp://example.com/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('javascript:alert(1)'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('data:text/html,x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('not a url'), isFalse);
    });

    test('rejects loopback and private IPv4', () {
      expect(SafeUrl.isSafeMediaUrl('https://127.0.0.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://127.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://localhost/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://10.0.0.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://192.168.1.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://172.16.0.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://172.31.255.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://169.254.169.254/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://100.64.0.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://0.0.0.0/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://224.0.0.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://240.0.0.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://255.255.255.255/x'), isFalse);
    });

    test('rejects IPv6 private forms', () {
      expect(SafeUrl.isSafeMediaUrl('https://[::1]/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://[::]/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://[::ffff:127.0.0.1]/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://[::ffff:10.0.0.1]/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://[fc00::1]/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://[fe80::1]/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://[fd00::1]/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://[ff02::1]/x'), isFalse);
    });

    test('rejects encoded and shorthand IPv4', () {
      expect(SafeUrl.isSafeMediaUrl('https://0x7f000001/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://2130706433/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://0177.0.0.1/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://0/x'), isFalse);
    });

    test('rejects DNS rebinding domains', () {
      expect(SafeUrl.isSafeMediaUrl('https://x.nip.io/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://nip.io/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://x.10.0.0.1.xip.io/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://sslip.io/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://localtest.me/x'), isFalse);
      expect(SafeUrl.isSafeMediaUrl('https://x.loca.lt/x'), isFalse);
    });

    test('rejects embedded credentials', () {
      expect(SafeUrl.isSafeMediaUrl('https://user:pass@example.com/x'), isFalse);
    });
  });

  group('SafeUrl.isSafePlaybackUrl', () {
    test('allows the app local range server', () {
      expect(
        SafeUrl.isSafePlaybackUrl('http://127.0.0.1:8123/blob/${'a' * 64}'),
        isTrue,
      );
      expect(SafeUrl.isSafePlaybackUrl('http://127.0.0.1:8123/other'), isFalse);
      expect(SafeUrl.isSafePlaybackUrl('https://example.com/a.mp3'), isTrue);
    });
  });
}