import '../utils/json_ext.dart';

/// Shared base for {id, pubkey, content, createdAt} relay entries.
class SocialEntry {
  final String id;
  final String pubkey;
  final String content;
  final int createdAt;

  SocialEntry({
    required this.id,
    required this.pubkey,
    required this.content,
    required this.createdAt,
  });

  factory SocialEntry.fromJson(Map<String, dynamic> json) {
    return SocialEntry(
      id: json.strOf('id'),
      pubkey: json.strOf('pubkey'),
      content: json.strOf('content'),
      createdAt: json.intOf('created_at'),
    );
  }
}
