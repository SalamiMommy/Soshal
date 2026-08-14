/// Shared display-format helpers (pubkey truncation, timestamps).
library;

/// Truncate a pubkey to `6…6` when longer than 12 chars.
String shortPubkey(String pk) {
  if (pk.length <= 12) return pk;
  return '${pk.substring(0, 6)}…${pk.substring(pk.length - 6)}';
}

/// Local `yyyy-MM-dd HH:mm` timestamp; empty string when `unix <= 0`.
String formatTimestamp(int unix) {
  if (unix <= 0) return '';
  final local = DateTime.fromMillisecondsSinceEpoch(unix * 1000).toLocal();
  String two(int v) => v.toString().padLeft(2, '0');
  return '${local.year}-${two(local.month)}-${two(local.day)} '
      '${two(local.hour)}:${two(local.minute)}';
}

/// Relative time (`just now` / m / h / d ago); empty string when `unix <= 0`.
String relativeTime(int unix) {
  if (unix <= 0) return '';
  final diff = DateTime.now()
      .difference(DateTime.fromMillisecondsSinceEpoch(unix * 1000));
  if (diff.inDays > 0) return '${diff.inDays}d ago';
  if (diff.inHours > 0) return '${diff.inHours}h ago';
  if (diff.inMinutes > 0) return '${diff.inMinutes}m ago';
  return 'just now';
}
