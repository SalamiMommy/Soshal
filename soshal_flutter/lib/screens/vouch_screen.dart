import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/session_service.dart';
import '../services/vouch_service.dart';
import '../utils/format.dart';

/// Vouch: web-of-trust endorsements for a target pubkey.
class VouchScreen extends StatefulWidget {
  /// Vouch screen.
  const VouchScreen({super.key});

  @override
  State<VouchScreen> createState() => _VouchScreenState();
}

class _VouchScreenState extends State<VouchScreen> {
  final TextEditingController _targetController = TextEditingController();
  final TextEditingController _contentController = TextEditingController();
  bool _loading = false;
  String _status = '';

  @override
  void initState() {
    super.initState();
    final pubkey = context.read<SessionService>().activePubkey;
    if (pubkey != null) {
      _targetController.text = pubkey;
      _load();
    }
  }

  @override
  void dispose() {
    _targetController.dispose();
    _contentController.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final target = _targetController.text.trim();
    if (target.isEmpty) return;
    setState(() => _loading = true);
    try {
      await context.read<VouchService>().fetch(target);
    } catch (e) {
      debugPrint('vouch load: $e');
      if (mounted) setState(() => _status = 'Fetch failed: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _publish() async {
    final target = _targetController.text.trim();
    final content = _contentController.text.trim();
    if (target.isEmpty || content.isEmpty) {
      setState(() => _status = 'Target pubkey and content required.');
      return;
    }
    setState(() => _status = 'Publishing...');
    try {
      final id = await context.read<VouchService>().publish(target, content);
      if (!mounted) return;
      _contentController.clear();
      setState(() =>
          _status = 'Published ${prefixEllipsis(id, 16, ellipsis: '...')}');
      await _load();
    } catch (e) {
      if (mounted) setState(() => _status = 'Failed: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Vouch')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          TextField(
            controller: _targetController,
            decoration: const InputDecoration(
              labelText: 'Target pubkey',
              hintText: 'hex or npub',
              border: OutlineInputBorder(),
            ),
            onChanged: (_) => setState(() => _status = ''),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _contentController,
            maxLines: 3,
            decoration: const InputDecoration(
              labelText: 'Endorsement',
              hintText: 'What do you vouch for?',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              FilledButton.icon(
                onPressed: _publish,
                icon: const Icon(Icons.verified_outlined),
                label: const Text('Publish Vouch'),
              ),
              const SizedBox(width: 8),
              OutlinedButton.icon(
                onPressed: _load,
                icon: const Icon(Icons.refresh),
                label: const Text('Refresh'),
              ),
            ],
          ),
          if (_status.isNotEmpty) ...[
            const SizedBox(height: 8),
            Text(_status,
                style: Theme.of(context)
                    .textTheme
                    .bodySmall
                    ?.copyWith(color: Theme.of(context).colorScheme.outline)),
          ],
          const Divider(height: 32),
          Text(
              'Vouches for ${prefixEllipsis(_targetController.text.trim(), 16, ellipsis: '...')}',
              style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          if (_loading)
            const Center(
                child: Padding(
              padding: EdgeInsets.all(16),
              child: CircularProgressIndicator(),
            ))
          else
            Consumer<VouchService>(
              builder: (context, service, _) {
                if (service.vouches.isEmpty) {
                  return const Padding(
                    padding: EdgeInsets.symmetric(vertical: 16),
                    child: Text(
                        'No vouches yet. Enter a target pubkey and publish one.'),
                  );
                }
                return Column(
                  children: service.vouches.map((v) {
                    return Card(
                      margin: const EdgeInsets.symmetric(vertical: 4),
                      child: Padding(
                        padding: const EdgeInsets.all(12),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(prefixEllipsis(v.pubkey, 16, ellipsis: '...'),
                                style: const TextStyle(
                                    fontWeight: FontWeight.bold, fontSize: 13)),
                            if (v.content.isNotEmpty) ...[
                              const SizedBox(height: 4),
                              SelectableText(v.content),
                            ],
                            if (v.createdAt > 0) ...[
                              const SizedBox(height: 4),
                              Text(
                                formatTimestamp(v.createdAt),
                                style: Theme.of(context)
                                    .textTheme
                                    .bodySmall
                                    ?.copyWith(
                                        color: Theme.of(context)
                                            .colorScheme
                                            .outline),
                              ),
                            ],
                          ],
                        ),
                      ),
                    );
                  }).toList(),
                );
              },
            ),
        ],
      ),
    );
  }
}
