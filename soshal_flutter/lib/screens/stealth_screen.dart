import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/stealth_service.dart';

/// Stealth: whitelist of pubkeys allowed to see you when stealth mode is
/// active. Stored locally.
class StealthScreen extends StatefulWidget {
  /// Stealth screen.
  const StealthScreen({super.key});

  @override
  State<StealthScreen> createState() => _StealthScreenState();
}

class _StealthScreenState extends State<StealthScreen> {
  final TextEditingController _whitelistController = TextEditingController();
  bool _loading = true;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _whitelistController.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final items = await context.read<StealthService>().load();
      if (mounted) _whitelistController.text = items.join('\n');
    } catch (e) {
      debugPrint('stealth load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _save() async {
    final items = _whitelistController.text
        .split('\n')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .toList();
    setState(() => _saving = true);
    try {
      final ok = await context.read<StealthService>().save(items);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: SelectableText(ok ? 'Whitelist saved.' : 'Save failed.'),
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    }
    if (mounted) setState(() => _saving = false);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      appBar: AppBar(title: const Text('Stealth Whitelist')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : ListView(
              padding: const EdgeInsets.all(16),
              children: [
                Text(
                  'Pubkeys allowed to see you when stealth mode is active. '
                  'Stored locally.',
                  style: theme.textTheme.bodyMedium
                      ?.copyWith(color: theme.colorScheme.outline),
                ),
                const SizedBox(height: 16),
                TextField(
                  controller: _whitelistController,
                  maxLines: 8,
                  decoration: const InputDecoration(
                    hintText: 'One pubkey per line (hex or npub)',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 16),
                FilledButton.icon(
                  onPressed: _saving ? null : _save,
                  icon: const Icon(Icons.save_outlined),
                  label: Text(_saving ? 'Saving...' : 'Save Whitelist'),
                ),
              ],
            ),
    );
  }
}
