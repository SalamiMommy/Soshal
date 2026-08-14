import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'dart:io';
import 'package:video_player/video_player.dart';
import '../services/bookmarks_service.dart';
import '../services/feed_service.dart';
import '../services/moderation_service.dart';
import '../services/media_service.dart';
import '../services/p2p_service.dart';
import '../services/session_service.dart';
import 'composer_screen.dart';
import '../services/zap_service.dart';
import '../services/layout_service.dart';
import '../widgets/error_state_text.dart';

/// Feed Page
/// Paginated posts, infinite scroll
class FeedScreen extends StatefulWidget {
  const FeedScreen({super.key});

  @override
  State<FeedScreen> createState() => _FeedScreenState();
}

class _FeedScreenState extends State<FeedScreen> {
  late ScrollController _scrollController;
  FeedService? _feed;

  @override
  void initState() {
    super.initState();
    _scrollController = ScrollController();
    _scrollController.addListener(_onScroll);

    // Load initial feed
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final feed = _feed;
      if (feed == null) return;
      feed.fetchFeed();
      _scheduleLayout(feed);
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _feed = context.read<FeedService>();
  }

  void _scheduleLayout(FeedService feed) {
    if (!mounted || !context.mounted) return;
    final size = MediaQuery.sizeOf(context);
    final layout = context.read<LayoutService>();
    layout.refresh(
      feed.posts,
      screenWidth: size.width.round(),
      textScale: MediaQuery.textScalerOf(context).scale(14),
    );
    feed.addListener(_onFeedChanged);
  }

  void _onFeedChanged() {
    if (!mounted || !context.mounted) return;
    final size = MediaQuery.sizeOf(context);
    context.read<LayoutService>().refresh(
          context.read<FeedService>().posts,
          screenWidth: size.width.round(),
          textScale: MediaQuery.textScalerOf(context).scale(14),
        );
  }

  @override
  void dispose() {
    _feed?.removeListener(_onFeedChanged);
    _scrollController.dispose();
    super.dispose();
  }

  void _onScroll() {
    if (_scrollController.position.pixels ==
        _scrollController.position.maxScrollExtent) {
      // Load more when scrolling to bottom
      context.read<FeedService>().loadMore();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Soshal'),
        elevation: 0,
        actions: [
          IconButton(
            icon: const Icon(Icons.person),
            onPressed: () {
              final sessionService = context.read<SessionService>();
              if (sessionService.activePubkey != null) {
                context.go('/profile/${sessionService.activePubkey}');
              }
            },
          ),
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: () => context.go('/settings'),
          ),
        ],
      ),
      body: Consumer<FeedService>(
        builder: (context, feedService, child) {
          if (feedService.isLoading && feedService.posts.isEmpty) {
            return const Center(child: CircularProgressIndicator());
          }

          if (feedService.posts.isEmpty) {
            return Center(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Text('No posts yet'),
                  const SizedBox(height: 16),
                  ElevatedButton(
                    onPressed: () {
                      feedService.fetchFeed();
                    },
                    child: const Text('Refresh'),
                  ),
                ],
              ),
            );
          }

          return ListView.builder(
            controller: _scrollController,
            itemCount: feedService.posts.length + 1,
            itemExtentBuilder: (index, _) => context
                .read<LayoutService>()
                .extentFor(index, feedService.posts),
            itemBuilder: (context, index) {
              if (index == feedService.posts.length) {
                if (feedService.isLoading) {
                  return const Padding(
                    padding: EdgeInsets.all(16),
                    child: Center(child: CircularProgressIndicator()),
                  );
                }
                return const SizedBox.shrink();
              }

              final post = feedService.posts[index];
              return FeedPostCard(post: post);
            },
          );
        },
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: () {
          showModalBottomSheet(
            context: context,
            isScrollControlled: true,
            builder: (context) => const ComposerScreen(),
          );
        },
        child: const Icon(Icons.edit),
      ),
      bottomNavigationBar: BottomNavigationBar(
        items: const [
          BottomNavigationBarItem(icon: Icon(Icons.home), label: 'Feed'),
          BottomNavigationBarItem(icon: Icon(Icons.mail), label: 'Messages'),
          BottomNavigationBarItem(
              icon: Icon(Icons.notifications), label: 'Notifications'),
          BottomNavigationBarItem(icon: Icon(Icons.person), label: 'Profile'),
        ],
        onTap: (index) {
          switch (index) {
            case 1:
              context.go('/inbox');
              break;
            case 2:
              context.go('/notifications');
              break;
            case 3:
              final sessionService = context.read<SessionService>();
              if (sessionService.activePubkey != null) {
                context.go('/profile/${sessionService.activePubkey}');
              }
              break;
          }
        },
      ),
    );
  }
}

/// Feed Post Card
class FeedPostCard extends StatefulWidget {
  final FeedPost post;

  const FeedPostCard({super.key, required this.post});

  @override
  State<FeedPostCard> createState() => _FeedPostCardState();
}

class _FeedPostCardState extends State<FeedPostCard> {
  late bool _liked;

  int _totalMsat = 0;

  @override
  void initState() {
    super.initState();
    _liked = widget.post.liked;
    _loadTotal();
  }

  Future<void> _loadTotal() async {
    try {
      _totalMsat =
          await context.read<ZapService>().fetchTotalMsat(widget.post.eventId);
      if (mounted) setState(() {});
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.symmetric(vertical: 8, horizontal: 8),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Author header
            Row(
              children: [
                if (widget.post.profilePicture != null)
                  CircleAvatar(
                    backgroundImage: ResizeImage.resizeIfNeeded(
                        128, 128, NetworkImage(widget.post.profilePicture!)),
                    radius: 24,
                  )
                else
                  const CircleAvatar(radius: 24),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        widget.post.profileName ?? 'Anonymous',
                        style: const TextStyle(fontWeight: FontWeight.bold),
                      ),
                      Text(
                        widget.post.pubkey.substring(0, 16),
                        style: const TextStyle(
                          fontSize: 12,
                          color: Colors.grey,
                        ),
                      ),
                    ],
                  ),
                ),
                if (widget.post.pubkey ==
                    context.read<SessionService>().activePubkey)
                  PopupMenuButton<String>(
                    icon: const Icon(Icons.more_vert, size: 18),
                    tooltip: 'Post actions',
                    onSelected: (v) async {
                      if (v != 'delete') return;
                      final doIt = await showDialog<bool>(
                        context: context,
                        builder: (context) => AlertDialog(
                          title: const Text('Delete post?'),
                          content: const Text(
                              'Removes the post and its search index '
                              'entry. This cannot be undone.'),
                          actions: [
                            TextButton(
                              onPressed: () => Navigator.pop(context, false),
                              child: const Text('Cancel'),
                            ),
                            FilledButton(
                              onPressed: () => Navigator.pop(context, true),
                              child: const Text('Delete'),
                            ),
                          ],
                        ),
                      );
                      if (doIt != true) return;
                      if (!context.mounted) return;
                      try {
                        await context.read<FeedService>().deletePost(
                            widget.post.eventId,
                            context.read<SessionService>().activePubkey!);
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(
                                content: SelectableText('Post deleted')),
                          );
                        }
                        if (!context.mounted) return;
                        context.read<FeedService>().fetchFeed();
                      } catch (e) {
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                              content: SelectableText('Delete error: $e')));
                        }
                      }
                    },
                    itemBuilder: (context) => const [
                      PopupMenuItem(
                        value: 'delete',
                        child: Text('Delete'),
                      ),
                    ],
                  )
                else
                  PopupMenuButton<String>(
                    icon: const Icon(Icons.more_vert, size: 18),
                    tooltip: 'Post actions',
                    onSelected: (v) => _onModAction(v),
                    itemBuilder: (context) => const [
                      PopupMenuItem(
                        value: 'bookmark',
                        child: Text('Bookmark'),
                      ),
                      PopupMenuItem(
                        value: 'mute',
                        child: Text('Mute user'),
                      ),
                      PopupMenuItem(
                        value: 'block',
                        child: Text('Block user'),
                      ),
                      PopupMenuItem(
                        value: 'report',
                        child: Text('Report post'),
                      ),
                    ],
                  ),
              ],
            ),
            const SizedBox(height: 12),
            // Content
            Text(
              widget.post.content,
              style: const TextStyle(fontSize: 14),
            ),
            const SizedBox(height: 12),
            // Media attachments
            if (widget.post.media != null) _buildMediaCard(),
            const SizedBox(height: 12),
            // Reactions
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceEvenly,
              children: [
                _buildReactionButton(
                    Icons.favorite, _liked ? Colors.red : Colors.grey, () {
                  setState(() => _liked = !_liked);
                  context.read<FeedService>().createReaction(
                        widget.post.eventId,
                        _liked ? '+' : '-',
                        context.read<SessionService>().activePubkey ?? '',
                      );
                }, label: '${widget.post.reactions}'),
                _buildReactionButton(
                  Icons.chat_bubble_outline,
                  Colors.grey,
                  () {
                    context.go('/post/${widget.post.eventId}');
                  },
                  label: '${widget.post.replies}',
                ),
                _buildReactionButton(
                  Icons.mood,
                  Colors.grey,
                  _showEmojiPicker,
                ),
                _buildReactionButton(Icons.bolt, Colors.amber, () {
                  _showZapDialog();
                }),
                if (_totalMsat > 0)
                  Padding(
                    padding: const EdgeInsets.only(left: 8, top: 8),
                    child: Text(
                      '${(_totalMsat / 1000).toStringAsFixed(2)} sats',
                      style: const TextStyle(fontSize: 11, color: Colors.amber),
                    ),
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _showZapDialog() async {
    final zap = context.read<ZapService>();
    var connected = false;
    try {
      connected = zap.isConnected;
    } catch (_) {}
    if (!connected) {
      final uri = TextEditingController();
      final ok = await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Connect NWC wallet'),
          content: TextField(
            controller: uri,
            decoration: const InputDecoration(
              labelText: 'nostr+walletconnect:// URI',
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Connect'),
            ),
          ],
        ),
      );
      if (ok != true || !mounted) return;
      try {
        await zap.connect(uri.text.trim());
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: SelectableText('Connect failed: $e')));
        }
        return;
      }
    }
    await zap.fetchReceipts(widget.post.eventId);
    await zap.fetchTotalMsat(widget.post.eventId);
    if (!mounted) return;
    showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Zaps'),
        content: SizedBox(
          width: double.maxFinite,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('Total: ${(zap.totalMsat / 1000).toStringAsFixed(2)} sats',
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              if (zap.receipts.isEmpty)
                const Text('No receipts yet')
              else
                for (final r in zap.receipts)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 2),
                    child: Text(
                      '⚡ ${(r.amountMsat / 1000).toStringAsFixed(2)} sats · '
                      '${r.zapperPubkey.substring(0, 10)}…',
                      style: const TextStyle(fontSize: 13),
                    ),
                  ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  Widget _buildReactionButton(
      IconData icon, Color color, VoidCallback onPressed,
      {String? label}) {
    return Expanded(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: Icon(icon),
            color: color,
            onPressed: onPressed,
          ),
          if (label != null)
            Text(
              label,
              style: const TextStyle(fontSize: 11, color: Colors.grey),
            ),
        ],
      ),
    );
  }

  void _snack(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: SelectableText(message)));
  }

  Future<void> _showEmojiPicker() async {
    final pubkey = context.read<SessionService>().activePubkey;
    if (pubkey == null) {
      _snack('Sign in to react');
      return;
    }
    final emoji = await showModalBottomSheet<String>(
      context: context,
      builder: (context) => Padding(
        padding: const EdgeInsets.all(16),
        child: Wrap(
          spacing: 16,
          runSpacing: 8,
          children: [
            for (final e in const ['👍', '❤️', '😂', '😮', '😢', '😡'])
              IconButton(
                icon: Text(e, style: const TextStyle(fontSize: 24)),
                onPressed: () => Navigator.pop(context, e),
              ),
          ],
        ),
      ),
    );
    if (emoji == null || !mounted) return;
    try {
      await context.read<FeedService>().createReaction(
            widget.post.eventId,
            emoji,
            pubkey,
          );
      _snack('Reacted $emoji');
    } catch (e) {
      _snack('Reaction error: $e');
    }
  }

  Future<void> _onModAction(String value) async {
    final session = context.read<SessionService>();
    final pubkey = session.activePubkey;
    if (pubkey == null) {
      _snack('Sign in');
      return;
    }
    switch (value) {
      case 'bookmark':
        await _toggleBookmark(pubkey);
      case 'mute':
        try {
          await context.read<ModerationService>().mute(
                pubkey,
                widget.post.pubkey,
              );
          _snack('User muted');
        } catch (e) {
          _snack('Mute error: $e');
        }
      case 'block':
        await _blockUser(pubkey);
      case 'report':
        await _reportPost(pubkey);
    }
  }

  Future<void> _toggleBookmark(String pubkey) async {
    try {
      final bookmarks = context.read<BookmarksService>();
      final rows = await bookmarks.list(pubkey);
      final existing =
          rows.where((r) => r.eventId == widget.post.eventId).toList();
      if (existing.isNotEmpty) {
        await bookmarks.delete(existing.first.id);
        _snack('Removed from Bookmarks');
      } else {
        await bookmarks.save(pubkey, widget.post.eventId);
        _snack('Saved to Bookmarks');
      }
    } catch (e) {
      _snack('Bookmark error: $e');
    }
  }

  Future<void> _blockUser(String pubkey) async {
    final doIt = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Block user?'),
        content: const Text(
            'Their posts will be hidden from you. This can be undone.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Block'),
          ),
        ],
      ),
    );
    if (doIt != true || !mounted) return;
    try {
      await context.read<ModerationService>().block(
            pubkey,
            widget.post.pubkey,
          );
      _snack('User blocked');
    } catch (e) {
      _snack('Block error: $e');
    }
  }

  Future<void> _reportPost(String pubkey) async {
    final reason = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Report post'),
        content: TextField(
          controller: reason,
          autofocus: true,
          decoration: const InputDecoration(
            hintText: 'Reason',
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Report'),
          ),
        ],
      ),
    );
    if (ok != true || !mounted) return;
    final text = reason.text.trim();
    if (text.isEmpty) {
      _snack('Reason required');
      return;
    }
    try {
      await context.read<ModerationService>().reportContent(
            reporterPubkey: pubkey,
            contentType: 'post',
            contentId: widget.post.eventId,
            reason: text,
          );
      _snack('Report filed');
    } catch (e) {
      _snack('Report error: $e');
    }
  }

  Widget _buildMediaCard() {
    final media = widget.post.media!;
    if (media.type == 'image') {
      return _BlobImage(url: media.url, blobHash: media.blobHash);
    } else if (media.type == 'video') {
      return _VideoPlayerWidget(url: media.url, blobHash: media.blobHash);
    }
    return const SizedBox.shrink();
  }
}

/// Image with the same blob + LAN-crawl fallback as `_VideoPlayerWidget`:
/// when the URL isn't this device's own server and the post carries a CAS
/// blob hash, fetch (local first, then discovered LAN peers) and serve from
/// the local range server. Honest "unavailable" box on failure.
class _BlobImage extends StatefulWidget {
  final String url;
  final String? blobHash;

  const _BlobImage({required this.url, this.blobHash});

  @override
  State<_BlobImage> createState() => _BlobImageState();
}

class _BlobImageState extends State<_BlobImage> {
  String? _resolved;
  String? _error;

  @override
  void initState() {
    super.initState();
    _prepare();
  }

  Future<void> _prepare() async {
    var url = widget.url;
    final hash = widget.blobHash;
    final host = Uri.parse(url).host;
    final isLocal = host.isEmpty ||
        host == 'localhost' ||
        host == '127.0.0.1' ||
        host == '::1';
    if (hash != null && !isLocal) {
      final media = context.read<MediaService>();
      final transport = context.read<P2pService>();
      try {
        if (media.localServerPort == null) await media.startLocalServer();
        try {
          await media.fetchBlob(hash);
        } catch (_) {
          await media.fetchBlobFromLan(
            hash,
            peers: transport.peers,
            outPath: '${Directory.systemTemp.path}/$hash',
          );
        }
        url = media.getLocalUrl(hash);
      } catch (e) {
        if (mounted) setState(() => _error = '$e');
      }
    }
    if (mounted) setState(() => _resolved = url);
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return Container(
        height: 200,
        color: Colors.grey[300],
        child: Center(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: ErrorStateText('Image unavailable: $_error'),
          ),
        ),
      );
    }
    final url = _resolved;
    if (url == null) {
      return Container(
        height: 200,
        color: Colors.grey[300],
        child: const Center(child: CircularProgressIndicator()),
      );
    }
    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: Image.network(
        url,
        cacheWidth: 800,
        fit: BoxFit.cover,
        errorBuilder: (context, error, stackTrace) {
          return Container(
            height: 200,
            color: Colors.grey[300],
            child: const Center(
              child: Text('Failed to load image'),
            ),
          );
        },
      ),
    );
  }
}

class _VideoPlayerWidget extends StatefulWidget {
  final String url;
  final String? blobHash;

  const _VideoPlayerWidget({required this.url, this.blobHash});

  @override
  State<_VideoPlayerWidget> createState() => _VideoPlayerWidgetState();
}

class _VideoPlayerWidgetState extends State<_VideoPlayerWidget> {
  VideoPlayerController? _controller;
  bool _isInitialized = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _prepare();
  }

  /// Resolve the playback URL: if the post references a CAS blob and the
  /// remote URL isn't this device's own server, fetch the blob (local store
  /// first, then a LAN crawl of discovered peers) and play from the local
  /// range server. Honest failure: no peers / no local copy = error UI.
  Future<void> _prepare() async {
    var url = widget.url;
    final hash = widget.blobHash;
    final host = Uri.parse(url).host;
    final isLocal = host.isEmpty ||
        host == 'localhost' ||
        host == '127.0.0.1' ||
        host == '::1';
    if (hash != null && !isLocal) {
      final media = context.read<MediaService>();
      final transport = context.read<P2pService>();
      try {
        if (media.localServerPort == null) await media.startLocalServer();
        try {
          await media.fetchBlob(hash);
        } catch (_) {
          await media.fetchBlobFromLan(
            hash,
            peers: transport.peers,
            outPath: '${Directory.systemTemp.path}/$hash',
          );
        }
        url = media.getLocalUrl(hash);
      } catch (e) {
        if (mounted) setState(() => _error = '$e');
      }
    }
    final controller = VideoPlayerController.networkUrl(Uri.parse(url));
    _controller = controller;
    await controller.initialize();
    if (mounted) setState(() => _isInitialized = true);
  }

  @override
  void dispose() {
    _controller?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return Container(
        height: 200,
        color: Colors.grey[300],
        child: Center(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: ErrorStateText('Video unavailable: $_error'),
          ),
        ),
      );
    }
    if (!_isInitialized || _controller == null) {
      return Container(
        height: 200,
        color: Colors.grey[300],
        child: const Center(
          child: CircularProgressIndicator(),
        ),
      );
    }
    final controller = _controller!;

    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: AspectRatio(
        aspectRatio: controller.value.aspectRatio,
        child: Stack(
          alignment: Alignment.center,
          children: [
            VideoPlayer(controller),
            VideoProgressIndicator(controller, allowScrubbing: true),
            Positioned(
              right: 8,
              bottom: 8,
              child: IconButton(
                icon: Icon(
                  controller.value.isPlaying ? Icons.pause : Icons.play_arrow,
                  color: Colors.white,
                ),
                onPressed: () {
                  setState(() {
                    if (controller.value.isPlaying) {
                      controller.pause();
                    } else {
                      controller.play();
                    }
                  });
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
