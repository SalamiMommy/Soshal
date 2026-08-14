import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../services/minis_service.dart';

/// Minis user page: lists fetched minis. Mini URLs do not carry author
/// information, so this shows all fetched minis with the author in context.
class MinisUserScreen extends StatefulWidget {
  /// Author pubkey being viewed.
  final String pubkey;

  /// Minis user screen.
  const MinisUserScreen({super.key, required this.pubkey});

  @override
  State<MinisUserScreen> createState() => _MinisUserScreenState();
}

class _MinisUserScreenState extends State<MinisUserScreen> {
  List<String> _minis = [];
  bool _loading = true;

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
      debugPrint('minis user load: $e');
    }
    if (mounted) setState(() => _loading = false);
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
    return Scaffold(
      appBar: AppBar(title: const Text('Minis')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : RefreshIndicator(
              onRefresh: _load,
              child: _minis.isEmpty
                  ? ListView(
                      children: [
                        const SizedBox(height: 120),
                        Center(
                          child: Padding(
                            padding: const EdgeInsets.all(24),
                            child: Column(
                              children: [
                                Icon(
                                  Icons.apps,
                                  size: 56,
                                  color: Theme.of(context).colorScheme.outline,
                                ),
                                const SizedBox(height: 16),
                                Text('No minis yet',
                                    style:
                                        Theme.of(context).textTheme.titleLarge),
                                const SizedBox(height: 8),
                                Text(
                                  'No minis available. Mini URLs carry no author, so this list shows the full fetched registry.',
                                  textAlign: TextAlign.center,
                                  style: TextStyle(
                                      color: Theme.of(context)
                                          .colorScheme
                                          .onSurfaceVariant),
                                ),
                              ],
                            ),
                          ),
                        ),
                      ],
                    )
                  : ListView.separated(
                      itemCount: _minis.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, index) {
                        final url = _minis[index];
                        return ListTile(
                          leading: Icon(
                            Icons.apps,
                            color: Theme.of(context).colorScheme.primary,
                          ),
                          title: Text(
                            url,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle:
                              Text('Mini app URL · viewed by ${widget.pubkey}'),
                          trailing: const Icon(Icons.open_in_new),
                          onTap: () => _openMini(url),
                        );
                      },
                    ),
            ),
    );
  }
}
