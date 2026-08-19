extension JsonMap on Map<String, dynamic> {
  String strOf(String key) => this[key] as String? ?? '';
  int intOf(String key) => (this[key] as num?)?.toInt() ?? 0;
  bool boolOf(String key) => this[key] as bool? ?? false;
  String? strOrNull(String key) => this[key] as String?;
}
