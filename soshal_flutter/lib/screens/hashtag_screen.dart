import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/search_service.dart';

/// Hashtag page: posts tagged with a given hashtag.
class HashtagScreen extends StatefulWidget {
  /// Hashtag name (without the leading '#').
  final String hashtag;

  /// Hashtag screen.
  const HashtagScreen({super.key, required this.hashtag});

  @override
  State<HashtagScreen> createState() => _HashtagScreenState();
}

class _HashtagScreenState extends State<HashtagScreen> {
  bool _loading = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      await context
          .read<SearchService>()
          .searchPosts('#${widget.hashtag}', limit: 50);
    } catch (e) {
      debugPrint('hashtag posts: $e');
      if (mounted) setState(() => _error = e.toString());
    }
    if (mounted) setState(() => _loading = false);
  }

  String _shortPk(String pk) {
    if (pk.length < 12) return pk;
    return '${pk.substring(0, 6)}…${pk.substring(pk.length - 6)}';
  }

  String _formatTime(int unix) {
    final local = DateTime.fromMillisecondsSinceEpoch(unix * 1000).toLocal();
    String two(int v) => v.toString().padLeft(2, '0');
    return '${local.year}-${two(local.month)}-${two(local.day)} '
        '${two(local.hour)}:${two(local.minute)}';
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: Text('#${widget.hashtag}'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            onPressed: _loading ? null : _load,
          ),
        ],
      ),
      body: Consumer<SearchService>(
        builder: (context, api, _) {
          if (_loading) {
            return const Center(child: CircularProgressIndicator());
          }
          if (_error != null) {
            return Center(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Text(
                  'Error: $_error',
                  style: TextStyle(color: colors.error),
                ),
              ),
            );
          }
          final posts = api.results
              .where((r) => r.kind == 'post' || r.kind.isEmpty)
              .toList();
          if (posts.isEmpty) {
            return Center(
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.tag, size: 48, color: Colors.grey),
                    const SizedBox(height: 12),
                    Text('No posts tagged #${widget.hashtag} yet'),
                    const SizedBox(height: 4),
                    const Text(
                      'Posts matching this tag will appear here.',
                      style: TextStyle(color: Colors.grey),
                    ),
                  ],
                ),
              ),
            );
          }
          return ListView.builder(
            itemCount: posts.length,
            itemBuilder: (context, index) {
              final p = posts[index];
              final pubkey = p.pubkey ?? '';
              return Card(
                margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                child: ListTile(
                  leading: CircleAvatar(
                    radius: 16,
                    child: Text(
                      pubkey.isNotEmpty
                          ? _shortPk(pubkey).substring(0, 1)
                          : '?',
                      style: const TextStyle(fontSize: 12),
                    ),
                  ),
                  title: Text(
                    p.description,
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                  ),
                  subtitle: Text(
                    '${_shortPk(pubkey)} · ${_formatTime(p.createdAt)}',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  onTap: () {
                    if (p.id.isNotEmpty) context.go('/post/${p.id}');
                  },
                ),
              );
            },
          );
        },
      ),
    );
  }
}
