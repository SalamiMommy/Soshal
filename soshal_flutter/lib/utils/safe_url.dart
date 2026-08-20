import 'dart:io' show InternetAddress, InternetAddressType;

/// Dart mirror of `common-core/src/url.rs` media-URL safety checks.
///
/// Blocks loopback/private/link-local/CGNAT/multicast/reserved hosts, DNS
/// rebinding domains, encoded IPv4 forms and embedded credentials — anything
/// a relay-supplied URL could use to reach internal services (SSRF). Every
/// `Image.network` / `NetworkImage` / player `setUrl` call that touches
/// untrusted data must pass `isSafeMediaUrl` first.
class SafeUrl {
  static const int maxLength = 2048;

  static final RegExp _blocked = RegExp(
    r'localhost|127\.0\.0\.1|^127\.|0x7f|169\.254|192\.168\.|10\.|'
    r'172\.(1[6-9]|2[0-9]|3[0-1])\.|::1|fc00:|fe80:|^0\.0\.0\.0$|^0$|'
    r'^0[0-7]+\.|^0x[0-9a-f]+\.|^\d{5,}',
    caseSensitive: false,
  );

  static final RegExp _rebinding = RegExp(
    r'(\.nip\.io|\.xip\.io|\.sslip\.io|\.localtest\.me|\.loca\.lt|'
    r'\.customer\.your-server\.de|\.dns\.to|\.traefik\.me)$|'
    r'^(nip\.io|xip\.io|sslip\.io|localtest\.me|loca\.lt)$',
    caseSensitive: false,
  );

  static final RegExp _hexHost = RegExp(r'^0x[0-9a-f]+$', caseSensitive: false);

  static final RegExp _localRangeServer =
      RegExp(r'^http://127\.0\.0\.1:\d+/blob/[0-9a-f]{64}$');

  /// True when `source` is the app's own local range server (used to play
  /// CAS blobs fetched from the chunk store or LAN peers).
  static bool isLocalRangeServer(String source) {
    return _localRangeServer.hasMatch(source.trim());
  }

  /// Playback variant of [isSafeMediaUrl]: additionally allows the app's own
  /// local range server so blob playback keeps working.
  static bool isSafePlaybackUrl(String source) {
    if (isLocalRangeServer(source)) return true;
    return isSafeMediaUrl(source);
  }

  /// True only for http(s) URLs with a public host. Blob refs
  /// (`blob://<hash>`, `n<hash>`, bare hex) are NOT media URLs — handle them
  /// before calling this.
  static bool isSafeMediaUrl(String source) {
    final s = source.trim();
    if (s.isEmpty || s.length > maxLength) return false;
    final uri = Uri.tryParse(s);
    if (uri == null) return false;
    if (uri.scheme != 'http' && uri.scheme != 'https') return false;
    if (uri.userInfo.isNotEmpty) return false;
    final host = uri.host.toLowerCase();
    if (host.isEmpty) return false;

    final ip = InternetAddress.tryParse(host);
    if (ip != null) return !_isPrivateIp(ip);

    if (_blocked.hasMatch(host)) return false;
    if (_rebinding.hasMatch(host)) return false;
    if (_hexHost.hasMatch(host)) return false;
    return true;
  }

  static bool _isPrivateIp(InternetAddress ip) {
    if (ip.type == InternetAddressType.IPv4) {
      return _isPrivateV4(ip.address);
    }
    final a = ip.address.toLowerCase();
    if (a == '::' || a == '::1' || a.startsWith('ff')) return true;
    if (a.startsWith('fe80') || a.startsWith('fc00') || a.startsWith('fd')) {
      return true;
    }
    if (a.startsWith('::ffff:')) {
      final v4 = InternetAddress.tryParse(a.substring('::ffff:'.length));
      if (v4 != null && v4.type == InternetAddressType.IPv4) {
        return _isPrivateV4(v4.address);
      }
    }
    return false;
  }

  static bool _isPrivateV4(String address) {
    final parts = address.split('.').map(int.tryParse).toList();
    if (parts.length != 4 || parts.any((p) => p == null || p < 0 || p > 255)) {
      return true;
    }
    final o = parts.cast<int>();
    if (o[0] == 0 || o[0] == 10 || o[0] == 127) return true;
    if (o[0] == 169 && o[1] == 254) return true;
    if (o[0] == 172 && o[1] >= 16 && o[1] <= 31) return true;
    if (o[0] == 192 && o[1] == 168) return true;
    if (o[0] == 100 && o[1] >= 64 && o[1] <= 127) return true;
    if (o[0] >= 224) return true;
    return false;
  }
}