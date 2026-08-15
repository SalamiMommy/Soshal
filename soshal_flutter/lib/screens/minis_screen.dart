import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../services/minis_service.dart';

/// Minis: mini-app registry. Each mini is a URL — tapping offers to open it
/// (copies the URL to the clipboard; there is no in-app webview).
class MinisScreen extends StatefulWidget {
  /// Minis screen.
  const MinisScreen({super.key});

  @override
  State<MinisScreen> createState() => _MinisScreenState();
}

class _MinisScreenState extends State<MinisScreen> {
  List<String> _minis = [];
  bool _loading = true;
  String _filterText = '';
  String? _filterResult;
  bool _rankOn = false;
  List<String> _ranked = [];

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      _minis = context.read<MinisService>().fetchMinis();
    } catch (e) {
      debugPrint('minis load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  void _runFilter() {
    final text = _filterText.trim();
    if (text.isEmpty) return;
    try {
      final result = context.read<MinisService>().runFilter(
            pluginId: 'content-filter',
            text: text,
            wasmBytesHex: '',
          );
      if (!mounted) return;
      setState(() => _filterResult = result);
    } catch (e) {
      if (mounted) setState(() => _filterResult = 'error: $e');
    }
  }

  Future<void> _toggleRank(bool on) async {
    setState(() => _rankOn = on);
    if (!on) {
      setState(() => _ranked = []);
      return;
    }
    if (_minis.isEmpty) return;
    try {
      final posts = _minis.map((u) => jsonEncode({'url': u})).toList();
      final ranked = context.read<MinisService>().rankFeed(
            pluginId: 'feed-ranker',
            postsJson: posts,
            wasmBytesHex: '',
          );
      if (!mounted) return;
      setState(() => _ranked = ranked);
    } catch (e) {
      debugPrint('rank feed: $e');
      if (mounted) {
        setState(() {
          _rankOn = false;
          _ranked = [];
        });
      }
    }
  }

  void _openMini(String url) {
    showModalBottomSheet<void>(
      context: context,
      builder: (sheetContext) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Mini', style: Theme.of(sheetContext).textTheme.titleLarge),
              const SizedBox(height: 8),
              Text(
                url,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(sheetContext).textTheme.bodyMedium,
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: url));
                  Navigator.of(sheetContext).pop();
                  if (!context.mounted) return;
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: Text('Mini URL copied to clipboard')),
                  );
                },
                icon: const Icon(Icons.open_in_new),
                label: const Text('Open mini'),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final display = _rankOn && _ranked.isNotEmpty ? _ranked : _minis;
    return Scaffold(
      appBar: AppBar(title: const Text('Minis')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : RefreshIndicator(
              onRefresh: _load,
              child: ListView(
                padding: const EdgeInsets.all(16),
                children: [
                  Text('Content filter plugin',
                      style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 8),
                  TextField(
                    onChanged: (v) => setState(() => _filterText = v),
                    decoration: const InputDecoration(
                      hintText: 'Text to check with the WASI filter plugin',
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 8),
                  FilledButton.icon(
                    icon: const Icon(Icons.filter_alt),
                    label: const Text('Run filter'),
                    onPressed: _runFilter,
                  ),
                  if (_filterResult != null) ...[
                    const SizedBox(height: 8),
                    Text(
                      _filterResult!,
                      maxLines: 4,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 12),
                    ),
                  ],
                  const Divider(height: 32),
                  SwitchListTile(
                    title: const Text('Rank feed with mini plugin'),
                    subtitle: const Text(
                        'Reorders the mini list via the WASI feed-ranker host'),
                    value: _rankOn,
                    onChanged: _toggleRank,
                  ),
                  if (_rankOn) ...[
                    const Text(
                      'Ranked order',
                      style: TextStyle(fontSize: 11, color: Colors.grey),
                    ),
                    const SizedBox(height: 4),
                  ],
                  const Divider(height: 32),
                  Text('Minis', style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 8),
                  if (display.isEmpty)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 24),
                      child: Column(
                        children: [
                          Icon(
                            Icons.apps,
                            size: 56,
                            color: Theme.of(context).colorScheme.outline,
                          ),
                          const SizedBox(height: 16),
                          Text('No minis yet',
                              style: Theme.of(context).textTheme.titleLarge),
                          const SizedBox(height: 8),
                          Text(
                            'The mini registry is empty on the backend right now. Minis are not installed on-device in this build.',
                            textAlign: TextAlign.center,
                            style: TextStyle(
                                color: Theme.of(context)
                                    .colorScheme
                                    .onSurfaceVariant),
                          ),
                        ],
                      ),
                    )
                  else
                    for (var i = 0; i < display.length; i++) ...[
                      ListTile(
                        leading: Icon(
                          Icons.apps,
                          color: Theme.of(context).colorScheme.primary,
                        ),
                        title: Text(
                          display[i],
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        subtitle: Text(
                          _rankOn && _ranked.isNotEmpty
                              ? 'Mini app URL · rank ${i + 1}'
                              : 'Mini app URL',
                        ),
                        trailing: const Icon(Icons.open_in_new),
                        onTap: () => _openMini(display[i]),
                      ),
                      if (i < display.length - 1) const Divider(height: 1),
                    ],
                ],
              ),
            ),
    );
  }
}
