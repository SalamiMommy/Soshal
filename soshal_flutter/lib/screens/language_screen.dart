// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Language setting — persists a locale code in the `language` settings key.
/// The legacy UI offered a locale list; the app itself renders English for
/// now, and this screen records the preference honestly.
class LanguageScreen extends StatefulWidget {
  const LanguageScreen({super.key});

  @override
  State<LanguageScreen> createState() => _LanguageScreenState();
}

class _LanguageScreenState extends State<LanguageScreen> {
  static const locales = <(String, String)>[
    ('en', 'English'),
    ('de', 'Deutsch'),
    ('fr', 'Français'),
    ('es', 'Español'),
    ('pt', 'Português'),
    ('ja', '日本語'),
    ('zh', '中文'),
  ];

  String _current = 'en';

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final saved =
          RustLib.instance.api.crateFfiDbDbGetSetting(key: 'language');
      if (saved != null && saved.isNotEmpty && mounted) {
        setState(() => _current = saved);
      }
    } catch (_) {}
  }

  Future<void> _save(String code) async {
    try {
      RustLib.instance.api.crateFfiDbDbSetSetting(
        key: 'language',
        value: code,
      );
    } catch (e) {
      debugPrint('save language: $e');
    }
    if (mounted) setState(() => _current = code);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Language')),
      body: ListView(
        children: [
          for (final (code, label) in locales)
            ListTile(
              title: Text(label),
              trailing: _current == code ? const Icon(Icons.check) : null,
              onTap: () => _save(code),
            ),
          const Padding(
            padding: EdgeInsets.all(16),
            child: Text(
              'UI localization pending; the preference is stored and applied '
              'when translations land.',
              style: TextStyle(fontStyle: FontStyle.italic),
            ),
          ),
        ],
      ),
    );
  }
}
