// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/settings_service.dart';
import '../services/shell_service.dart';
import '../services/stealth_service.dart';
import '../widgets/settings_scaffold.dart';

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
    } catch (e) {
      debugPrint('privacy load: $e');
    }
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
    return SettingsScaffold(
      title: 'Privacy',
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
            context.push('/settings/privacy/stealth');
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
    );
  }
}

class StealthEditorScreen extends StatefulWidget {
  const StealthEditorScreen({super.key});

  @override
  State<StealthEditorScreen> createState() => StealthEditorScreenState();
}

class StealthEditorScreenState extends State<StealthEditorScreen> {
  final TextEditingController _field = TextEditingController();
  bool _saved = false;

  @override
  void dispose() {
    _field.dispose();
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final items = await context.read<StealthService>().load();
      _field.text = items.join('\n');
    } catch (e) {
      debugPrint('privacy load: $e');
    }
  }

  Future<void> _save() async {
    try {
      final items = _field.text
          .split('\n')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();
      final ok = await context.read<StealthService>().save(items);
      if (ok) _saved = true;
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
  // Step 0: enter current PIN (only when hasPin is true)
  // Step 1: enter new PIN
  // Step 2: confirm new PIN
  int _step = 0;
  final TextEditingController _current = TextEditingController();
  final TextEditingController _newPin = TextEditingController();
  final TextEditingController _confirmPin = TextEditingController();
  String? _error;

  static const _inputDecoration = InputDecoration(
    hintText: '4-12 digits',
    border: OutlineInputBorder(),
  );

  @override
  void dispose() {
    _current.dispose();
    _newPin.dispose();
    _confirmPin.dispose();
    super.dispose();
  }

  String get _title {
    switch (_step) {
      case 0:
        return 'Enter current PIN';
      case 2:
        return 'Confirm new PIN';
      default:
        return 'Set a lock PIN';
    }
  }

  TextEditingController get _activeController {
    switch (_step) {
      case 0:
        return _current;
      case 2:
        return _confirmPin;
      default:
        return _newPin;
    }
  }

  Future<void> _onNext(BuildContext context) async {
    final shell = context.read<ShellService>();
    setState(() => _error = null);

    if (_step == 0) {
      // Verify current PIN before letting user enter a new one.
      if (_current.text.length < 4) {
        setState(() => _error = 'PIN must be at least 4 digits');
        return;
      }
      setState(() => _step = 1);
      return;
    }

    if (_step == 1) {
      if (_newPin.text.length < 4) {
        setState(() => _error = 'PIN must be at least 4 digits');
        return;
      }
      setState(() => _step = 2);
      return;
    }

    // Step 2: confirm
    if (_confirmPin.text != _newPin.text) {
      setState(() => _error = 'PINs do not match — try again');
      _confirmPin.clear();
      return;
    }
    try {
      if (shell.hasPin) {
        await shell.changePin(_current.text, _newPin.text);
      } else {
        await shell.setPin(_newPin.text);
      }
      if (context.mounted) {
        Navigator.of(context).pop();
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('PIN updated')),
        );
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('PIN change failed: $e')),
        );
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final shell = context.read<ShellService>();
    // Start at step 1 (new PIN) if no PIN is currently set.
    if (_step == 0 && !shell.hasPin) {
      _step = 1;
    }
    return AlertDialog(
      title: Text(_title),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextField(
            controller: _activeController,
            keyboardType: TextInputType.number,
            obscureText: true,
            maxLength: 12,
            decoration: _inputDecoration,
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => _onNext(context),
          child: Text(_step == 2 ? 'Confirm' : 'Next'),
        ),
        if (shell.hasPin && _step == 0)
          TextButton(
            onPressed: () async {
              await shell.clearPin(_current.text);
              if (context.mounted) Navigator.of(context).pop();
            },
            child: const Text('Clear PIN'),
          ),
      ],
    );
  }
}
