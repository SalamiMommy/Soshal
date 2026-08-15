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
      id: json['id'] as String? ?? '',
      pubkey: json['pubkey'] as String? ?? '',
      content: json['content'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
