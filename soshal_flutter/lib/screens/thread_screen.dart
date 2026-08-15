import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/feed_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';

/// Post thread screen: root post + replies + reply composer.
class ThreadScreen extends StatefulWidget {
  /// Post thread screen.
  const ThreadScreen({super.key, required this.eventId});

  final String eventId;

  @override
  State<ThreadScreen> createState() => _ThreadScreenState();
}

class _ThreadScreenState extends State<ThreadScreen> {
  bool _loading = true;
  bool _sending = false;
  List<FeedPost> _thread = [];
  List<ReactionSummary> _reactionSummary = [];
  final TextEditingController _replyController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _replyController.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final api = context.read<FeedService>();
      _thread = await api.fetchThread(widget.eventId);
      final session = context.read<SessionService>();
      _reactionSummary = api.aggregateChatReactions(
        _thread,
        session.activePubkey ?? '',
      );
    } catch (e) {
      debugPrint('thread load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _sendReply() async {
    final text = _replyController.text.trim();
    if (text.isEmpty) return;
    setState(() => _sending = true);
    try {
      final feed = context.read<FeedService>();
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) {
        throw Exception('Sign in to reply');
      }
      await feed.publishReply(
        text,
        widget.eventId,
        widget.eventId,
        pubkey,
      );
      _replyController.clear();
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Reply error: $e')));
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Thread')),
      body: Column(
        children: [
          Expanded(
            child: _loading
                ? const Center(child: CircularProgressIndicator())
                : Consumer<FeedService>(
                    builder: (context, feed, _) {
                      if (_thread.isEmpty) {
                        return const Center(child: Text('Thread not found'));
                      }
                      return ListView.builder(
                        itemCount: _thread.length,
                        itemBuilder: (context, index) {
                          final post = _thread[index];
                          final isRoot = index == 0;
                          return ListTile(
                            title: Text(
                              post.profileName ?? firstChars(post.pubkey, 12),
                              style:
                                  const TextStyle(fontWeight: FontWeight.bold),
                            ),
                            subtitle: isRoot
                                ? Column(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.start,
                                    children: [
                                      Text(post.content),
                                      if (_reactionSummary.isNotEmpty)
                                        Padding(
                                          padding:
                                              const EdgeInsets.only(top: 6),
                                          child: Wrap(
                                            spacing: 6,
                                            runSpacing: 4,
                                            children: [
                                              for (final s in _reactionSummary)
                                                Chip(
                                                  visualDensity:
                                                      VisualDensity.compact,
                                                  label: Text(
                                                    '${s.emoji} ${s.count}',
                                                    style: const TextStyle(
                                                        fontSize: 12),
                                                  ),
                                                ),
                                            ],
                                          ),
                                        ),
                                    ],
                                  )
                                : Text(post.content),
                            trailing: isRoot
                                ? Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      IconButton(
                                        icon: const Icon(Icons.favorite_border,
                                            size: 18),
                                        onPressed: () async {
                                          try {
                                            final session =
                                                context.read<SessionService>();
                                            final pubkey = session.activePubkey;
                                            if (pubkey == null) return;
                                            await context
                                                .read<FeedService>()
                                                .createReaction(
                                                    post.eventId, '+', pubkey);
                                            await _load();
                                          } catch (e) {
                                            debugPrint('react: $e');
                                          }
                                        },
                                      ),
                                      Text('${post.reactions}'),
                                      const SizedBox(width: 8),
                                      Text('↩ ${post.replies}'),
                                    ],
                                  )
                                : Text('↩ ${post.replies}'),
                          );
                        },
                      );
                    },
                  ),
          ),
          Container(
            padding: const EdgeInsets.all(8),
            child: Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _replyController,
                    decoration: InputDecoration(
                      hintText: 'Reply to thread',
                      border: OutlineInputBorder(
                        borderRadius: BorderRadius.circular(24),
                      ),
                      contentPadding: const EdgeInsets.symmetric(
                        horizontal: 16,
                        vertical: 8,
                      ),
                    ),
                    enabled: !_sending,
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  icon: const Icon(Icons.send),
                  onPressed: _sending ? null : _sendReply,
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
