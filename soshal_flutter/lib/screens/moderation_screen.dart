import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/moderation_service.dart';
import '../services/session_service.dart';

/// Moderation Settings
/// Muted users, word filters, and a content-filter test.
class ModerationScreen extends StatefulWidget {
  const ModerationScreen({super.key});

  @override
  State<ModerationScreen> createState() => _ModerationScreenState();
}

class _ModerationScreenState extends State<ModerationScreen> {
  String? _pubkey;
  String _testContent = '';
  bool? _filtered;

  @override
  void initState() {
    super.initState();
    _pubkey = context.read<SessionService>().activePubkey;
    context.read<ModerationService>().load(_pubkey ?? '');
  }

  Future<void> _addFilter() async {
    final controller = TextEditingController();
    final value = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Add word filter'),
        content: TextField(
          controller: controller,
          decoration: const InputDecoration(hintText: 'word or phrase'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, controller.text.trim()),
            child: const Text('Add'),
          ),
        ],
      ),
    );
    if (value == null || value.isEmpty) return;
    if (!mounted) return;
    final api = context.read<ModerationService>();
    await api.setWordFilters([...api.wordFilters, value]);
  }

  Future<void> _removeFilter(String filter) async {
    final api = context.read<ModerationService>();
    await api
        .setWordFilters(api.wordFilters.where((f) => f != filter).toList());
  }

  @override
  Widget build(BuildContext context) {
    final api = context.read<ModerationService>();
    return Scaffold(
      appBar: AppBar(title: const Text('Moderation')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('Muted users', style: Theme.of(context).textTheme.titleMedium),
          Consumer<ModerationService>(
            builder: (context, mod, _) {
              if (mod.muted.isEmpty) {
                return const Padding(
                  padding: EdgeInsets.symmetric(vertical: 8),
                  child: Text('No muted users'),
                );
              }
              return Column(
                children: [
                  for (final muted in mod.muted)
                    ListTile(
                      dense: true,
                      leading: const Icon(Icons.volume_off),
                      title: Text(muted,
                          maxLines: 1, overflow: TextOverflow.ellipsis),
                      trailing: IconButton(
                        icon: const Icon(Icons.undo),
                        tooltip: 'Unmute',
                        onPressed: () async {
                          final pubkey = _pubkey;
                          if (pubkey == null) return;
                          await mod.unmute(pubkey, muted);
                        },
                      ),
                    ),
                ],
              );
            },
          ),
          const Divider(height: 32),
          Row(
            children: [
              Text('Word filters',
                  style: Theme.of(context).textTheme.titleMedium),
              const Spacer(),
              IconButton(
                icon: const Icon(Icons.add),
                tooltip: 'Add filter',
                onPressed: _addFilter,
              ),
            ],
          ),
          Consumer<ModerationService>(
            builder: (context, mod, _) {
              if (mod.wordFilters.isEmpty) {
                return const Padding(
                  padding: EdgeInsets.symmetric(vertical: 8),
                  child: Text('No word filters'),
                );
              }
              return Wrap(
                spacing: 8,
                children: [
                  for (final f in mod.wordFilters)
                    Chip(
                      label: Text(f),
                      onDeleted: () => _removeFilter(f),
                    ),
                ],
              );
            },
          ),
          const Divider(height: 32),
          Text('Content check', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          TextField(
            onChanged: (v) => setState(() => _testContent = v),
            decoration: const InputDecoration(
              hintText: 'Paste content to test against filters',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 8),
          FilledButton.icon(
            icon: const Icon(Icons.filter_alt),
            label: const Text('Check content'),
            onPressed: () async {
              final pubkey = _pubkey;
              if (pubkey == null) return;
              final filtered = await api.shouldFilter(_testContent, pubkey);
              if (mounted) setState(() => _filtered = filtered);
            },
          ),
          if (_filtered != null)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Text(
                _filtered! ? '⚠ Filtered' : 'OK — passes filters',
                style: TextStyle(
                  color: _filtered! ? Colors.orange : Colors.green,
                  fontWeight: FontWeight.bold,
                ),
              ),
            ),
        ],
      ),
    );
  }
}
