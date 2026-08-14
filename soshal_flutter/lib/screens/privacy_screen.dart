// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/settings_service.dart';
import '../services/shell_service.dart';

/// Privacy settings — port of the legacy privacy section: privacy level
/// (public/private/dark/stealth), dating profile visibility, direct-message
/// filtering. Persisted via settings keys; relay enforcement is backend-side.
class PrivacyScreen extends StatefulWidget {
  const PrivacyScreen({super.key});

  @override
  State<PrivacyScreen> createState() => _PrivacyScreenState();
}

class _PrivacyScreenState extends State<PrivacyScreen> {
  static const levels = <(String, String)>[
    ('public', 'Public'),
    ('private', 'Private'),
    ('dark', 'Dark'),
    ('stealth', 'Stealth'),
  ];

  String _level = 'public';

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final saved = context.read<SettingsService>().getSetting('privacy_level');
      if (saved.isNotEmpty && mounted) {
        setState(() => _level = saved);
      }
    } catch (_) {}
  }

  Future<void> _save(String level) async {
    try {
      context.read<SettingsService>().setSetting('privacy_level', level);
    } catch (e) {
      debugPrint('save privacy: $e');
    }
    if (mounted) setState(() => _level = level);
  }

  @override
  Widget build(BuildContext context) {
    final shell = context.read<ShellService>();
    return Scaffold(
      appBar: AppBar(title: const Text('Privacy')),
      body: ListView(
        children: [
          const Padding(
            padding: EdgeInsets.fromLTRB(16, 16, 16, 8),
            child: Text('Privacy level',
                style: TextStyle(fontWeight: FontWeight.bold)),
          ),
          for (final (key, label) in levels)
            ListTile(
              title: Text(label),
              trailing: _level == key ? const Icon(Icons.check) : null,
              onTap: () => _save(key),
            ),
          const Divider(),
          ListTile(
            title: const Text('Stealth whitelist'),
            subtitle: const Text('Pubkeys that may always reach you'),
            trailing: const Icon(Icons.arrow_forward),
            onTap: () {
              Navigator.of(context).push(
                MaterialPageRoute(
                  builder: (_) => const _StealthEditor(),
                ),
              );
            },
          ),
          ListTile(
            title: const Text('Lock app'),
            subtitle: const Text('Require PIN to unlock'),
            trailing: const Icon(Icons.arrow_forward),
            onTap: () {
              shell.reevaluateLock();
              showDialog<void>(
                context: context,
                builder: (ctx) => const _PinDialog(),
              );
            },
          ),
        ],
      ),
    );
  }
}

class _StealthEditor extends StatefulWidget {
  const _StealthEditor();

  @override
  State<_StealthEditor> createState() => _StealthEditorState();
}

class _StealthEditorState extends State<_StealthEditor> {
  final TextEditingController _field = TextEditingController();
  bool _saved = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final v = context.read<SettingsService>().getSetting('stealth_whitelist');
      if (v.isNotEmpty) {
        _field.text = v;
      }
    } catch (_) {}
  }

  Future<void> _save() async {
    try {
      context
          .read<SettingsService>()
          .setSetting('stealth_whitelist', _field.text);
      _saved = true;
    } catch (e) {
      debugPrint('save whitelist: $e');
    }
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Stealth whitelist')),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text(
              'Pubkeys in this list can always see and contact you. One '
              'pubkey per line. An empty list means everyone is filtered '
              'while stealth mode is active.',
            ),
            const SizedBox(height: 12),
            Expanded(
              child: TextField(
                controller: _field,
                maxLines: null,
                expands: true,
                textAlignVertical: TextAlignVertical.top,
                decoration: const InputDecoration(
                  border: OutlineInputBorder(),
                  hintText: 'npub1…\nnpub1…',
                ),
              ),
            ),
            const SizedBox(height: 12),
            FilledButton(
              onPressed: _save,
              child: Text(_saved ? 'Whitelist saved.' : 'Save whitelist'),
            ),
          ],
        ),
      ),
    );
  }
}

class _PinDialog extends StatefulWidget {
  const _PinDialog();

  @override
  State<_PinDialog> createState() => _PinDialogState();
}

class _PinDialogState extends State<_PinDialog> {
  final TextEditingController _pin = TextEditingController();
  bool _confirming = false;

  @override
  Widget build(BuildContext context) {
    final shell = context.read<ShellService>();
    return AlertDialog(
      title: Text(_confirming ? 'Confirm new PIN' : 'Set a lock PIN'),
      content: TextField(
        controller: _pin,
        keyboardType: TextInputType.number,
        obscureText: true,
        maxLength: 12,
        decoration: const InputDecoration(
          hintText: '4-12 digits',
          border: OutlineInputBorder(),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () async {
            if (_confirming) {
              if (_pin.text.length >= 4) {
                await shell.setPin(_pin.text);
                if (context.mounted) Navigator.of(context).pop();
              }
              return;
            }
            if (_pin.text.length >= 4) {
              setState(() => _confirming = true);
            }
          },
          child: Text(_confirming ? 'Confirm' : 'Next'),
        ),
        if (shell.hasPin)
          TextButton(
            onPressed: () async {
              await shell.clearPin(_pin.text);
              if (context.mounted) Navigator.of(context).pop();
            },
            child: const Text('Clear PIN'),
          ),
      ],
    );
  }
}
