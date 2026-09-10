extension JsonMap on Map<String, dynamic> {
  String strOf(String key) => this[key] as String? ?? '';
  int intOf(String key) => (this[key] as num?)?.toInt() ?? 0;
  double doubleOf(String key) => (this[key] as num?)?.toDouble() ?? 0.0;
  bool boolOf(String key) => this[key] as bool? ?? false;
  String? strOrNull(String key) => this[key] as String?;

  List<String> stringsOf(String key) {
    final v = this[key];
    if (v is List) {
      if (v.isEmpty) return const [];
      return List<String>.generate(v.length, (i) => v[i]?.toString() ?? '');
    }
    return const [];
  }
}
