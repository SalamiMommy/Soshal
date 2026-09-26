/// Shared display-format helpers (pubkey truncation, timestamps).
///
/// Single canonical home for the snippets historically copy-pasted across
/// screens and services. Keep formatting logic here, not in widgets.
library;

import 'package:intl/intl.dart';

/// Truncate a pubkey/hex id to `head…tail` when longer than `minLen`.
///
/// Defaults to the common `6…6` style. Use `head`/`tail` for denser variants
/// (e.g. marketplace `8…4`, events `8…3`).
String shortPubkey(
  String pk, {
  int head = 6,
  int tail = 6,
  int minLen = 12,
}) {
  if (pk.length <= minLen) return pk;
  return '${pk.substring(0, head)}…${pk.substring(pk.length - tail)}';
}

/// First `n` chars, guarded (returns input unchanged when shorter).
String firstChars(String s, int n) => s.length <= n ? s : s.substring(0, n);

/// First `n` chars + ellipsis when truncated; input unchanged when shorter.
String prefixEllipsis(String s, int n, {String ellipsis = '…'}) =>
    s.length <= n ? s : '${s.substring(0, n)}$ellipsis';

/// Bytes as lowercase hex, each byte zero-padded to 2 chars.
String bytesToHex(List<int> bytes) {
  const chars = '0123456789abcdef';
  final buffer = StringBuffer();
  for (final b in bytes) {
    buffer.write(chars[(b >> 4) & 0x0f]);
    buffer.write(chars[b & 0x0f]);
  }
  return buffer.toString();
}

/// Local `yyyy-MM-dd HH:mm` timestamp; empty string when `unix <= 0`.
String formatTimestamp(int unix) {
  if (unix <= 0) return '';
  return formatDateTime(
      DateTime.fromMillisecondsSinceEpoch(unix * 1000).toLocal());
}

/// Local `yyyy-MM-dd HH:mm` from a [DateTime].
String formatDateTime(DateTime dt) {
  String two(int v) => v.toString().padLeft(2, '0');
  return '${dt.year}-${two(dt.month)}-${two(dt.day)} '
      '${two(dt.hour)}:${two(dt.minute)}';
}

/// 24h `HH:mm` wall clock.
String formatClock(DateTime t) =>
    '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';

/// 12h `h:mm AM/PM` wall clock.
String formatClock12h(DateTime t) {
  final period = t.hour >= 12 ? 'PM' : 'AM';
  final h12 = t.hour % 12 == 0 ? 12 : t.hour % 12;
  return '$h12:${t.minute.toString().padLeft(2, '0')} $period';
}

/// Short month names (`Jan`…`Dec`), indexed by `month - 1`.
const kMonthShort = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
];

/// `Jan 6 · 2:30 PM`-style label (short month, day, 12h clock).
String formatMonthDayTime(DateTime t) =>
    '${kMonthShort[t.month - 1]} ${t.day} · ${formatClock12h(t)}';

final _compactNumberFormat = NumberFormat.compact();
final _commaNumberFormat = NumberFormat('#,###');

/// Formats satoshi amounts cleanly, e.g. `1,500 sats`, `25K sats`, `1.2M sats`.
String formatSats(int sats) {
  if (sats <= 0) return '0 sats';
  if (sats >= 100000) {
    return '${_compactNumberFormat.format(sats)} sats';
  }
  return '${_commaNumberFormat.format(sats)} sats';
}

/// Relative time (`just now` / m / h / d ago); empty string when `unix <= 0`.
String relativeTime(int unix) => formatRelativeTime(unix);

/// Formats unix timestamp as relative social time ("just now", "5m ago", "2h ago", "yesterday", "3d ago")
/// falling back to DateFormat ("MMM d" or "MMM d, yyyy") for events older than a week.
String formatRelativeTime(int unix) {
  if (unix <= 0) return '';
  final now = DateTime.now();
  final date = DateTime.fromMillisecondsSinceEpoch(unix * 1000);
  final diff = now.difference(date);

  if (diff.isNegative || diff.inSeconds < 45) {
    return 'just now';
  }
  if (diff.inMinutes < 60) {
    return '${diff.inMinutes}m ago';
  }
  if (diff.inHours < 24) {
    return '${diff.inHours}h ago';
  }
  if (diff.inDays == 1) {
    return 'yesterday';
  }
  if (diff.inDays < 7) {
    return '${diff.inDays}d ago';
  }
  if (date.year == now.year) {
    return DateFormat.MMMd().format(date);
  }
  return DateFormat.yMMMd().format(date);
}
