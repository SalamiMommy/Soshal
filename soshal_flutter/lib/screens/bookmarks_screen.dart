import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/bookmarks_service.dart';
import '../services/feed_service.dart' show FeedPost;
import '../services/session_service.dart';
import '../widgets/error_state_text.dart';

/// Bookmarks: locally saved posts, resolved from the local DB cache.
class BookmarksScreen extends StatefulWidget {
  /// Bookmarks screen.
  const BookmarksScreen({super.key});

  @override
  State<BookmarksScreen> createState() => _BookmarksScreenState();
}

class _BookmarksScreenState extends State<BookmarksScreen> {
  final Map<String, FeedPost?> _posts = {};
  bool _loading = false;
  bool _loadingPosts = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final pubkey = context.read<SessionService>().activePubkey;
    if (pubkey == null) {
      if (mounted) setState(() => _error = 'No active account.');
      return;
    }
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      await context.read<BookmarksService>().list(pubkey);
      _resolvePosts();
    } catch (e) {
      debugPrint('bookmarks list: $e');
      if (mounted) setState(() => _error = e.toString());
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _resolvePosts() async {
    final api = context.read<BookmarksService>();
    final rows = api.bookmarks;
    setState(() => _loadingPosts = rows.isNotEmpty);
    for (final b in rows) {
      _posts[b.eventId] = await api.resolvePost(b.eventId);
      if (mounted) setState(() {});
    }
    if (mounted) setState(() => _loadingPosts = false);
  }

  Future<void> _delete(BookmarkRow b) async {
    try {
      await context.read<BookmarksService>().delete(b.id);
      _posts.remove(b.eventId);
      if (mounted) {
        setState(() {});
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Bookmark removed (${_shortPk(b.eventId)})')),
        );
      }
    } catch (e) {
      debugPrint('bookmark delete: $e');
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Delete failed: $e')));
      }
    }
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
    return Scaffold(
      appBar: AppBar(
        title: const Text('Bookmarks'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            onPressed: _loading ? null : _load,
          ),
        ],
      ),
      body: Consumer<BookmarksService>(
        builder: (context, api, _) {
          if (_loading) {
            return const Center(child: CircularProgressIndicator());
          }
          if (_error != null) {
            return Center(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: ErrorStateText('Error: $_error'),
              ),
            );
          }
          final rows = api.bookmarks;
          if (rows.isEmpty) {
            return const Center(
              child: Padding(
                padding: EdgeInsets.all(24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(Icons.bookmark_border, size: 48, color: Colors.grey),
                    SizedBox(height: 12),
                    Text('No bookmarks yet'),
                    SizedBox(height: 4),
                    Text(
                      'Saved posts will appear here.',
                      style: TextStyle(color: Colors.grey),
                    ),
                  ],
                ),
              ),
            );
          }
          if (_loadingPosts) {
            return const Center(child: CircularProgressIndicator());
          }
          return ListView.builder(
            itemCount: rows.length,
            itemBuilder: (context, index) {
              final b = rows[index];
              final post = _posts[b.eventId];
              return Card(
                margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                child: post == null
                    ? ListTile(
                        leading: const Icon(Icons.event_busy),
                        title: Text('event ${_shortPk(b.eventId)}'),
                        subtitle: const Text('event not cached locally'),
                        isThreeLine: false,
                        trailing: IconButton(
                          icon: const Icon(Icons.delete_outline),
                          onPressed: () => _delete(b),
                        ),
                      )
                    : ListTile(
                        leading: CircleAvatar(
                          radius: 16,
                          child: Text(
                            post.pubkey.isNotEmpty
                                ? _shortPk(post.pubkey).substring(0, 1)
                                : '?',
                            style: const TextStyle(fontSize: 12),
                          ),
                        ),
                        title: Text(
                          post.content.replaceAll('\n', ' '),
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                        ),
                        subtitle: Text(
                          '${_shortPk(post.pubkey)} · ${_formatTime(post.createdAt)}',
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                        trailing: IconButton(
                          icon: const Icon(Icons.delete_outline),
                          onPressed: () => _delete(b),
                        ),
                      ),
              );
            },
          );
        },
      ),
    );
  }
}
