import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'dart:async';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:flutter/rendering.dart' show ScrollCacheExtent;
import 'dart:convert';
import 'dart:io';

import 'package:media_kit/media_kit.dart' as mk;
import 'package:media_kit_video/media_kit_video.dart';
import '../services/permissions_service.dart';
import '../services/bookmarks_service.dart';
import '../services/feed_service.dart';
import '../services/moderation_service.dart';
import '../services/media_service.dart';
import '../services/p2p_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../utils/safe_url.dart';
import '../utils/dialog_guard.dart';
import 'composer_screen.dart';
import '../services/zap_service.dart';
import '../services/friends_service.dart';
import '../widgets/app_snack.dart';
import '../widgets/audience_filter_dropdown.dart';
import '../widgets/empty_state.dart';
import '../widgets/error_state_text.dart';

/// Narrow view of [FeedService] — rebuilds only when list identity or
/// loading flag change, not on every reaction/like notify.
typedef _FeedView = ({
  List<FeedPost> posts,
  List<FeedPost> display,
  bool loading,
});

/// Feed Page
/// Paginated posts, infinite scroll
class FeedScreen extends StatefulWidget {
  const FeedScreen({super.key});

  @override
  State<FeedScreen> createState() => _FeedScreenState();
}

class _FeedScreenState extends State<FeedScreen> {
  late ScrollController _scrollController;
  double? _lastScrollPixels;
  DateTime? _lastTelemetryAt;
  FeedService? _feed;
  bool _isLoadingMore = false;
  final ValueNotifier<Map<String, int>> _totalsNotifier =
      ValueNotifier<Map<String, int>>({});
  AudienceFilter _audienceFilter = AudienceFilter.all;
  String _selectedFeedTab = 'All';
  final List<String> _feedTabs = const [
    'All',
    'Favorites',
    'Friends',
    'Groups'
  ];

  @override
  void initState() {
    super.initState();
    _scrollController = ScrollController();
    _scrollController.addListener(_onScroll);

    // Load initial feed
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (!mounted) return;
      final myPk = context.read<SessionService>().activePubkey;
      if (myPk != null) {
        context.friendsServiceReadOrNull?.loadAudienceGraph(myPk);
      }
      final feed = _feed;
      if (feed == null) return;
      try {
        await feed.fetchFeed();
        await _loadTotals();
      } catch (e) {
        debugPrint('feed load: $e');
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Feed load error: $e')));
      }
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _feed = context.read<FeedService>();
  }

  Future<void> _loadTotals() async {
    final feed = _feed;
    if (!mounted || feed == null) return;
    // Fetch only ids we haven't seen yet — refetching the whole page on
    // every refresh/loadMore re-queries the DB for already-known totals.
    final currentTotals = _totalsNotifier.value;
    final missing = feed.displayPosts
        .where((p) => !currentTotals.containsKey(p.eventId))
        .toList();
    if (missing.isEmpty) return;
    final ids = missing.map((p) => p.eventId).toList();
    try {
      final totals = await context.read<ZapService>().fetchTotals(ids);
      if (!mounted) return;
      _totalsNotifier.value = {..._totalsNotifier.value, ...totals};
    } catch (e) {
      debugPrint('feed totals: $e');
    }
  }

  /// Manual refresh — only path that refetches the feed from offset 0. The
  /// live sync stream does NOT mutate the feed anymore (manual-refresh UI),
  /// so cards stay put until this runs.
  Future<void> _refreshFeed() async {
    final feed = _feed;
    if (feed == null) return;
    try {
      await feed.fetchFeed();
      await _loadTotals();
    } catch (e) {
      debugPrint('feed refresh: $e');
      if (!mounted || !context.mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Feed load error: $e')));
    }
  }

  @override
  void dispose() {
    _scrollController.dispose();
    _totalsNotifier.dispose();
    super.dispose();
  }

  /// Approx visible window from scroll offset + viewport height; used to
  /// gate blob/video preparation to cards near the viewport (cacheExtent).
  static const double _avgCardHeight = 400;

  bool _isIndexVisible(int index) {
    final pos = _scrollController.hasClients ? _scrollController.offset : 0.0;
    final h = _scrollController.hasClients
        ? _scrollController.position.viewportDimension
        : _avgCardHeight;
    final first = (pos / _avgCardHeight).floor() - 1;
    final last = ((pos + h) / _avgCardHeight).ceil() + 1;
    return index >= first && index <= last;
  }

  void _onScroll() async {
    if (!mounted) return;
    final media = context.read<MediaService>();
    final pos = _scrollController.position;
    final now = DateTime.now();
    if (_lastTelemetryAt == null ||
        now.difference(_lastTelemetryAt!) >=
            const Duration(milliseconds: 200)) {
      final dt = _lastTelemetryAt != null
          ? now.difference(_lastTelemetryAt!).inMicroseconds / 1000000.0
          : 0.2;
      _lastTelemetryAt = now;
      final delta = pos.pixels - (_lastScrollPixels ?? pos.pixels);
      final velocity = dt > 0 ? delta / dt : 0.0;
      media.updateScrollTelemetry(
        velocity: velocity,
        topIndex: (pos.pixels / 400).floor().clamp(0, 1 << 30),
        bottomIndex: (pos.pixels / 400).floor() + 2,
      );
    }
    _lastScrollPixels = pos.pixels;
    if (pos.pixels >= pos.maxScrollExtent - 400) {
      if (_isLoadingMore) return;
      _isLoadingMore = true;
      try {
        await context.read<FeedService>().loadMore();
        await _loadTotals();
      } catch (e) {
        debugPrint('feed loadMore: $e');
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Load more error: $e')));
      } finally {
        _isLoadingMore = false;
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final feedView = context.select<FeedService, _FeedView>(
        (f) => (posts: f.posts, display: f.displayPosts, loading: f.isLoading));
    final allDisplay = feedView.display;
    final friendsService = context.friendsServiceOrNull;
    final myPk = context.select<SessionService, String?>((s) => s.activePubkey);
    final audiencePosts = friendsService != null
        ? friendsService.filterList(
            allDisplay,
            _audienceFilter,
            (p) => p.pubkey,
            myPubkey: myPk,
          )
        : allDisplay;
    final List<FeedPost> filteredPosts = switch (_selectedFeedTab) {
      'Favorites' =>
        audiencePosts.where((p) => p.reactions > 0 || p.liked).toList(),
      'Friends' =>
        audiencePosts.where((p) => p.reposts > 0 || p.reactions > 0).toList(),
      'Groups' => audiencePosts
          .where((p) =>
              p.content.contains('#group') || p.content.contains('group'))
          .toList(),
      _ => audiencePosts,
    };
    final effectiveDisplay = filteredPosts.isEmpty && _selectedFeedTab != 'All'
        ? audiencePosts
        : filteredPosts;

    final Widget body;
    if (feedView.loading && feedView.posts.isEmpty) {
      body = const Center(child: CircularProgressIndicator());
    } else if (feedView.posts.isEmpty) {
      body = Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const Text('No posts yet'),
            const SizedBox(height: 16),
            ElevatedButton(
              onPressed: _refreshFeed,
              child: const Text('Refresh'),
            ),
          ],
        ),
      );
    } else if (effectiveDisplay.isEmpty) {
      body = EmptyState(
        icon: Icons.filter_list_off,
        title: 'No posts match this filter',
        body: 'Try switching your feed tab or audience filter to see more posts.',
        action: ElevatedButton(
          onPressed: _refreshFeed,
          child: const Text('Refresh'),
        ),
      );
    } else {
      body = ListView.builder(
        scrollCacheExtent: const ScrollCacheExtent.pixels(600.0),
        controller: _scrollController,
        itemCount: effectiveDisplay.length + 1,
        itemBuilder: (context, index) {
          if (index == effectiveDisplay.length) {
            if (feedView.loading) {
              return const Padding(
                padding: EdgeInsets.all(16),
                child: Center(child: CircularProgressIndicator()),
              );
            }
            return const SizedBox.shrink();
          }

          final post = effectiveDisplay[index];
          return RepaintBoundary(
            child: FeedPostCard(
              key: ValueKey(post.eventId),
              post: post,
              totalsNotifier: _totalsNotifier,
              isFirst: index == 0,
              visible: _isIndexVisible(index),
            ),
          );
        },
      );
    }
    return Scaffold(
      appBar: AppBar(
        title: const Text('Soshal'),
        elevation: 0,
        actions: [
          AudienceFilterDropdown(
            value: _audienceFilter,
            onChanged: (val) => setState(() => _audienceFilter = val),
          ),
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: 'Refresh',
            onPressed: _refreshFeed,
          ),
          Consumer<FeedService>(
            builder: (context, feed, _) => IconButton(
              icon:
                  Icon(feed.isRanked ? Icons.auto_awesome : Icons.access_time),
              tooltip: feed.isRanked
                  ? 'Top Posts (Algorithmic)'
                  : 'Most Recent (Chronological)',
              color:
                  feed.isRanked ? Theme.of(context).colorScheme.primary : null,
              onPressed: () {
                context.read<FeedService>().toggleRanking();
              },
            ),
          ),
          IconButton(
            icon: const Icon(Icons.person),
            onPressed: () {
              final sessionService = context.read<SessionService>();
              if (sessionService.activePubkey != null) {
                context.push('/profile/${sessionService.activePubkey}');
              }
            },
          ),
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: () => context.go('/settings'),
          ),
        ],
      ),
      body: Column(
        children: [
          Container(
            height: 44,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            child: ListView.separated(
              scrollDirection: Axis.horizontal,
              itemCount: _feedTabs.length,
              separatorBuilder: (_, __) => const SizedBox(width: 8),
              itemBuilder: (context, i) {
                final tab = _feedTabs[i];
                final selected = _selectedFeedTab == tab;
                return ChoiceChip(
                  label: Text(tab),
                  selected: selected,
                  onSelected: (val) {
                    if (val) {
                      setState(() => _selectedFeedTab = tab);
                    }
                  },
                );
              },
            ),
          ),
          const Divider(height: 1),
          Expanded(child: body),
        ],
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
    );
  }
}

/// Feed Post Card
class FeedPostCard extends StatefulWidget {
  final FeedPost post;
  final Map<String, int>? totals;
  final ValueNotifier<Map<String, int>>? totalsNotifier;
  final bool isFirst;
  final bool visible;

  const FeedPostCard({
    super.key,
    required this.post,
    this.totals,
    this.totalsNotifier,
    this.isFirst = false,
    this.visible = true,
  });

  @override
  State<FeedPostCard> createState() => _FeedPostCardState();
}

class _FeedPostCardState extends State<FeedPostCard> {
  late bool _liked;
  late String _preview;

  int _totalMsat = 0;

  static String _truncateContent(String raw) {
    if (raw.length <= 320) return raw;
    final chars = raw.characters;
    if (chars.skip(320).isNotEmpty) {
      return '${chars.take(320)}…';
    }
    return raw;
  }

  @override
  void initState() {
    super.initState();
    _liked = widget.post.liked;
    final raw = widget.post.content;
    _preview = _truncateContent(raw);
    widget.totalsNotifier?.addListener(_onTotalsChanged);
    _loadTotal();
  }

  void _onTotalsChanged() {
    final notifier = widget.totalsNotifier;
    if (notifier == null) return;
    final val = notifier.value[widget.post.eventId];
    if (val != null && val != _totalMsat) {
      if (mounted) setState(() => _totalMsat = val);
    }
  }

  @override
  void didUpdateWidget(FeedPostCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.totalsNotifier != widget.totalsNotifier) {
      oldWidget.totalsNotifier?.removeListener(_onTotalsChanged);
      widget.totalsNotifier?.addListener(_onTotalsChanged);
    }
    if (oldWidget.post.eventId != widget.post.eventId ||
        oldWidget.post.content != widget.post.content ||
        oldWidget.post.liked != widget.post.liked ||
        oldWidget.totals != widget.totals) {
      _liked = widget.post.liked;
      final raw = widget.post.content;
      _preview = _truncateContent(raw);
      _loadTotal();
    }
  }

  @override
  void dispose() {
    widget.totalsNotifier?.removeListener(_onTotalsChanged);
    super.dispose();
  }

  Future<void> _loadTotal() async {
    final notifier = widget.totalsNotifier;
    if (notifier != null && notifier.value.containsKey(widget.post.eventId)) {
      _totalMsat = notifier.value[widget.post.eventId] ?? 0;
      return;
    }
    final totals = widget.totals;
    if (totals != null) {
      _totalMsat = totals[widget.post.eventId] ?? 0;
      return;
    }
    try {
      _totalMsat =
          await context.read<ZapService>().fetchTotalMsat(widget.post.eventId);
      if (mounted) setState(() {});
    } catch (e) {
      debugPrint('zap total: $e');
    }
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
                if (widget.post.profilePicture != null &&
                    SafeUrl.isSafeMediaUrl(widget.post.profilePicture!))
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
                        prefixEllipsis(widget.post.pubkey, 16),
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
                      final doIt = await showDialogDeferred<bool>(
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
                        child: Row(
                          children: [
                            Icon(Icons.bookmark_outline, size: 18),
                            SizedBox(width: 8),
                            Text('Bookmark'),
                          ],
                        ),
                      ),
                      PopupMenuItem(
                        value: 'snooze',
                        child: Row(
                          children: [
                            Icon(Icons.snooze, size: 18),
                            SizedBox(width: 8),
                            Text('Snooze user for 30 days'),
                          ],
                        ),
                      ),
                      PopupMenuItem(
                        value: 'hide',
                        child: Row(
                          children: [
                            Icon(Icons.visibility_off_outlined, size: 18),
                            SizedBox(width: 8),
                            Text('Hide post'),
                          ],
                        ),
                      ),
                      PopupMenuItem(
                        value: 'mute',
                        child: Row(
                          children: [
                            Icon(Icons.volume_off_outlined, size: 18),
                            SizedBox(width: 8),
                            Text('Mute user'),
                          ],
                        ),
                      ),
                      PopupMenuItem(
                        value: 'block',
                        child: Row(
                          children: [
                            Icon(Icons.block, size: 18),
                            SizedBox(width: 8),
                            Text('Block user'),
                          ],
                        ),
                      ),
                      PopupMenuItem(
                        value: 'report',
                        child: Row(
                          children: [
                            Icon(Icons.flag_outlined, size: 18),
                            SizedBox(width: 8),
                            Text('Report post'),
                          ],
                        ),
                      ),
                    ],
                  ),
              ],
            ),
            const SizedBox(height: 12),
            // Content
            Text(
              _preview,
              style: const TextStyle(fontSize: 14),
            ),
            const SizedBox(height: 12),
            // Media attachments
            if (widget.post.media != null) _buildMediaCard(),
            const SizedBox(height: 12),
            // Reactions & options
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceEvenly,
              children: [
                _buildReactionButton(
                  Icons.favorite,
                  _liked ? Colors.red : Colors.grey,
                  () {
                    setState(() => _liked = !_liked);
                    context.read<FeedService>().createReaction(
                          widget.post.eventId,
                          _liked ? '+' : '-',
                          context.read<SessionService>().activePubkey ?? '',
                        );
                  },
                  label: '${widget.post.reactions}',
                  tooltip: _liked ? 'Unlike' : 'Like',
                ),
                _buildReactionButton(
                  Icons.chat_bubble_outline,
                  Colors.grey,
                  () {
                    context.push('/post/${widget.post.eventId}');
                  },
                  label: '${widget.post.replies}',
                  tooltip: 'Reply',
                ),
                _buildReactionButton(
                  Icons.share_outlined,
                  Colors.grey,
                  _showShareDialog,
                  label: '${widget.post.reposts}',
                  tooltip: 'Share',
                ),
                _buildReactionButton(
                  Icons.mood,
                  Colors.grey,
                  _showEmojiPicker,
                  tooltip: 'React',
                ),
                _buildReactionButton(
                  Icons.bolt,
                  Colors.amber,
                  () {
                    _showZapDialog();
                  },
                  label: _totalMsat > 0
                      ? '${(_totalMsat / 1000).toStringAsFixed(1)} sats'
                      : null,
                  tooltip: 'Zap',
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
    } catch (e) {
      debugPrint('nwc connect check: $e');
    }
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
      if (ok != true || !mounted) {
        uri.dispose();
        return;
      }
      try {
        await zap.connect(uri.text.trim());
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: SelectableText('Connect failed: $e')));
        }
        uri.dispose();
        return;
      }
      uri.dispose();
    }
    await zap.fetchReceipts(widget.post.eventId);
    await zap.fetchTotalMsat(widget.post.eventId);
    if (!mounted) return;
    final lnurl = TextEditingController();
    final amount = TextEditingController();
    var sending = false;
    showDialogDeferred<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
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
                        '${prefixEllipsis(r.zapperPubkey, 10)}',
                        style: const TextStyle(fontSize: 13),
                      ),
                    ),
                const Divider(),
                TextField(
                  controller: lnurl,
                  decoration: const InputDecoration(
                    labelText: 'Recipient LN address (lud16)',
                    hintText: 'name@domain.com',
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: amount,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(
                    labelText: 'Amount (sats)',
                  ),
                ),
                const SizedBox(height: 8),
                SizedBox(
                  width: double.maxFinite,
                  child: FilledButton.icon(
                    icon: sending
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.bolt, size: 18),
                    label: Text(sending ? 'Sending…' : 'Send zap'),
                    onPressed: sending
                        ? null
                        : () {
                            setDialogState(() => sending = true);
                            _sendZap(
                              zap,
                              lnurl.text.trim(),
                              amount.text.trim(),
                              setSending: (v) =>
                                  setDialogState(() => sending = v),
                              onDone: () async {
                                await zap.fetchReceipts(widget.post.eventId);
                                await zap.fetchTotalMsat(widget.post.eventId);
                              },
                            );
                          },
                  ),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () async {
                try {
                  await zap.disconnect();
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                          content: SelectableText('NWC wallet disconnected')),
                    );
                  }
                } catch (e) {
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                        content: SelectableText('Disconnect failed: $e')));
                  }
                }
              },
              child: const Text('Disconnect NWC'),
            ),
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Close'),
            ),
          ],
        ),
      ),
    ).then((_) {
      lnurl.dispose();
      amount.dispose();
    });
  }

  Future<void> _sendZap(
    ZapService zap,
    String lnurl,
    String amountSats, {
    required void Function(bool) setSending,
    required Future<void> Function() onDone,
  }) async {
    if (!mounted) return;
    if (lnurl.isEmpty) {
      _snack('LN address required');
      return;
    }
    final sats = int.tryParse(amountSats);
    if (sats == null || sats <= 0) {
      _snack('Enter a positive amount in sats');
      return;
    }
    try {
      await zap.parseLnurl(lnurl);
      final invoiceJson = await zap.fetchInvoice(
        lnurl: lnurl,
        amountMsat: sats * 1000,
        nostrEvent: widget.post.eventId,
      );
      final invoice = (jsonDecode(invoiceJson)
              as Map<String, dynamic>)['bolt11'] as String? ??
          '';
      if (invoice.isEmpty) {
        _snack('No invoice in response');
        return;
      }
      await zap.sendPayment(invoice);
      await onDone();
      if (mounted) {
        _snack('⚡ Zapped $sats sats');
      }
    } catch (e) {
      _snack('Zap error: $e');
    } finally {
      setSending(false);
    }
  }

  Widget _buildReactionButton(
      IconData icon, Color color, VoidCallback onPressed,
      {String? label, String? tooltip}) {
    return Expanded(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: Icon(icon),
            color: color,
            tooltip: tooltip,
            onPressed: onPressed,
          ),
          if (label != null)
            Text(
              label,
              style: TextStyle(
                fontSize: 11,
                color: color == Colors.amber ? Colors.amber : Colors.grey,
              ),
            ),
        ],
      ),
    );
  }

  Future<void> _showShareDialog() async {
    showModalBottomSheet<void>(
      context: context,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(16)),
      ),
      builder: (sheetContext) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              leading: const Icon(Icons.repeat),
              title: const Text('Repost to feed'),
              onTap: () async {
                Navigator.pop(sheetContext);
                final session = context.read<SessionService>();
                final pubkey = session.activePubkey;
                if (pubkey == null) {
                  _snack('Sign in to repost');
                  return;
                }
                try {
                  await context.read<FeedService>().publishTextNote(
                        'nostr:${widget.post.eventId}',
                        [
                          ['e', widget.post.eventId, '', 'mention'],
                          ['q', widget.post.eventId],
                        ],
                        pubkey,
                      );
                  _snack('Reposted to feed');
                } catch (e) {
                  _snack('Repost failed: $e');
                }
              },
            ),
            ListTile(
              leading: const Icon(Icons.copy),
              title: const Text('Copy post text'),
              onTap: () {
                Navigator.pop(sheetContext);
                Clipboard.setData(ClipboardData(text: widget.post.content));
                _snack('Post text copied to clipboard');
              },
            ),
            ListTile(
              leading: const Icon(Icons.link),
              title: const Text('Copy post ID'),
              onTap: () {
                Navigator.pop(sheetContext);
                Clipboard.setData(ClipboardData(text: widget.post.eventId));
                _snack('Post ID copied to clipboard');
              },
            ),
          ],
        ),
      ),
    );
  }

  void _snack(String message) {
    if (!mounted) return;
    showAppSnack(context, message);
  }

  Future<void> _showEmojiPicker() async {
    final pubkey = context.read<SessionService>().activePubkey;
    if (pubkey == null) {
      _snack('Sign in to react');
      return;
    }
    const reactions = [
      ('👍', 'Like'),
      ('❤️', 'Love'),
      ('🥰', 'Care'),
      ('😂', 'Haha'),
      ('😮', 'Wow'),
      ('😢', 'Sad'),
      ('😡', 'Angry'),
    ];
    final selected = await showModalBottomSheet<String>(
      context: context,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(24)),
      builder: (context) => Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 20),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.spaceEvenly,
          children: [
            for (final r in reactions)
              InkWell(
                onTap: () => Navigator.pop(context, r.$1),
                borderRadius: BorderRadius.circular(16),
                child: Padding(
                  padding: const EdgeInsets.all(6),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(r.$1, style: const TextStyle(fontSize: 32)),
                      const SizedBox(height: 4),
                      Text(
                        r.$2,
                        style: Theme.of(context)
                            .textTheme
                            .bodySmall
                            ?.copyWith(fontSize: 10),
                      ),
                    ],
                  ),
                ),
              ),
          ],
        ),
      ),
    );
    if (selected == null || !mounted) return;
    try {
      await context.read<FeedService>().createReaction(
            widget.post.eventId,
            selected,
            pubkey,
          );
      _snack('Reacted $selected');
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
      case 'snooze':
        _snack('Snoozed posts from this user for 30 days');
      case 'hide':
        _snack('Post hidden from feed');
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
    final doIt = await showDialogDeferred<bool>(
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
    final ok = await showDialogDeferred<bool>(
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
    if (ok != true || !mounted) {
      reason.dispose();
      return;
    }
    final text = reason.text.trim();
    reason.dispose();
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
      return _BlobImage(
          url: media.url,
          blobHash: media.blobHash,
          eager: widget.isFirst,
          postId: widget.post.eventId,
          visible: widget.visible);
    } else if (media.type == 'video') {
      return _VideoPlayerWidget(
          url: media.url,
          blobHash: media.blobHash,
          eager: widget.isFirst,
          postId: widget.post.eventId,
          visible: widget.visible);
    }
    return const SizedBox.shrink();
  }
}

/// Memoized blob-resolved URLs per (post id, media) — re-scrolls reuse the LAN/local
/// result instead of refetching. Bounded LRU eviction prevents memory leak.
final Map<String, String> _resolvedUrlCache = <String, String>{};
const int _maxResolvedUrlCacheSize = 256;

String _makeResolvedKey(String postId, String mediaId) => '$postId:$mediaId';

void _cacheResolvedUrl(String postId, String mediaId, String url) {
  if (_resolvedUrlCache.length >= _maxResolvedUrlCacheSize) {
    _resolvedUrlCache.remove(_resolvedUrlCache.keys.first);
  }
  _resolvedUrlCache[_makeResolvedKey(postId, mediaId)] = url;
}

/// Image with the same blob + LAN-crawl fallback as `_VideoPlayerWidget`:
/// when the URL isn't this device's own server and the post carries a CAS
/// blob hash, fetch (local first, then discovered LAN peers) and serve from
/// the local range server. Honest "unavailable" box on failure.
class _BlobImage extends StatefulWidget {
  final String url;
  final String? blobHash;
  final bool eager;
  final String postId;
  final bool visible;

  const _BlobImage(
      {required this.url,
      this.blobHash,
      this.eager = false,
      required this.postId,
      this.visible = false});

  @override
  State<_BlobImage> createState() => _BlobImageState();
}

class _BlobImageState extends State<_BlobImage> {
  String? _resolved;
  String? _error;
  bool _started = false;

  @override
  void initState() {
    super.initState();
    if (widget.eager || widget.visible) _start();
  }

  @override
  void didUpdateWidget(covariant _BlobImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!_started && widget.visible) _start();
  }

  void _start() {
    if (_started) return;
    _started = true;
    _prepare();
  }

  Future<void> _prepare() async {
    final mediaId = widget.blobHash ?? widget.url;
    final cached = _resolvedUrlCache[_makeResolvedKey(widget.postId, mediaId)];
    if (cached != null) {
      if (mounted) setState(() => _resolved = cached);
      return;
    }
    final String url;
    try {
      url = await _resolveBlobUrl(context, widget.url, widget.blobHash);
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
      return;
    }
    _cacheResolvedUrl(widget.postId, mediaId, url);
    if (mounted) setState(() => _resolved = url);
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return Container(
        height: 200,
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
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
      if (!_started && widget.visible) _start();
      return Container(
        height: 200,
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        child: Center(
          child: _started
              ? const CircularProgressIndicator()
              : const Icon(Icons.image_outlined, color: Colors.grey),
        ),
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
            color: Theme.of(context).colorScheme.surfaceContainerHighest,
            child: const Center(
              child: Text('Failed to load image'),
            ),
          );
        },
      ),
    );
  }
}

Future<String> _resolveBlobUrl(
  BuildContext context,
  String url,
  String? hash,
) async {
  final parsed = Uri.tryParse(url);
  if (parsed == null || parsed.scheme != 'http' && parsed.scheme != 'https') {
    return '';
  }
  // Post content is untrusted: never hand loopback/private-host URLs to
  // Image.network / the video player (local SSRF via crafted content).
  // Legit local media URLs are built by the app itself (getLocalUrl) and
  // returned below — raw content URLs with local hosts are blocked.
  if (!SafeUrl.isSafeMediaUrl(url)) return '';
  if (hash != null) {
    final media = context.read<MediaService>();
    final transport = context.read<P2pService>();
    if (media.localServerPort == null) await media.startLocalServer();
    final local = await media.fetchBlobQuiet(hash);
    if (local == null) {
      await media.fetchBlobFromLan(
        hash,
        peers: transport.peers,
        outPath: '${Directory.systemTemp.path}/$hash',
      );
    }
    return media.getLocalUrl(hash);
  }
  return url;
}

class _VideoPlayerWidget extends StatefulWidget {
  final String url;
  final String? blobHash;
  final bool eager;
  final String postId;
  final bool visible;

  const _VideoPlayerWidget(
      {required this.url,
      this.blobHash,
      this.eager = false,
      required this.postId,
      this.visible = false});

  @override
  State<_VideoPlayerWidget> createState() => _VideoPlayerWidgetState();
}

class _VideoPlayerWidgetState extends State<_VideoPlayerWidget> {
  VideoController? _controller;
  bool _isInitialized = false;
  String? _error;
  bool _started = false;

  static bool get _playbackSupported =>
      PermissionsService.isAndroid || PermissionsService.isLinux;

  @override
  void initState() {
    super.initState();
    if (widget.eager || widget.visible) _start();
  }

  Timer? _offscreenDisposeTimer;

  @override
  void didUpdateWidget(covariant _VideoPlayerWidget oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.visible) {
      _offscreenDisposeTimer?.cancel();
      _offscreenDisposeTimer = null;
      if (!_started) {
        _start();
      } else if (_isInitialized && _controller != null) {
        // Player is initialized and becoming visible again
      }
    } else if (_isInitialized && oldWidget.visible && _controller != null) {
      _controller!.player.pause();
      // Schedule resource release after 3 seconds off-screen to avoid decoder exhaustion
      _offscreenDisposeTimer?.cancel();
      _offscreenDisposeTimer = Timer(const Duration(seconds: 3), () {
        if (!mounted || widget.visible) return;
        setState(() {
          _controller?.player.dispose();
          _controller = null;
          _isInitialized = false;
          _started = false;
        });
      });
    }
  }

  void _start() {
    if (_started) return;
    _started = true;
    _prepare();
  }

  /// Resolve the playback URL: if the post references a CAS blob and the
  /// remote URL isn't this device's own server, fetch the blob (local store
  /// first, then a LAN crawl of discovered peers) and play from the local
  /// range server. Playback is media_kit (ExoPlayer/MediaCodec on Android,
  /// mpv on Linux) on every platform.
  /// Honest failure: no peers / no local copy = error UI.
  Future<void> _prepare() async {
    if (!_playbackSupported) {
      if (mounted) {
        setState(() => _error = 'Video playback is not supported on this '
            'platform.');
      }
      return;
    }
    final mediaId = widget.blobHash ?? widget.url;
    final cached = _resolvedUrlCache[_makeResolvedKey(widget.postId, mediaId)];
    var url = cached ?? widget.url;
    if (cached == null) {
      try {
        url = await _resolveBlobUrl(context, widget.url, widget.blobHash);
      } catch (e) {
        if (mounted) setState(() => _error = '$e');
        return;
      }
      _cacheResolvedUrl(widget.postId, mediaId, url);
    }
    if (!SafeUrl.isSafePlaybackUrl(url)) {
      if (mounted) {
        setState(() => _error = 'Video source rejected (unsafe host).');
      }
      return;
    }
    final player = mk.Player();
    final controller = VideoController(player);
    _controller = controller;
    try {
      await player.open(mk.Media(url), play: widget.visible);
      if (!mounted || !_started || _controller != controller) {
        player.dispose();
        return;
      }
      setState(() => _isInitialized = true);
    } catch (e) {
      player.dispose();
      if (mounted && _started && _controller == controller) {
        setState(() => _error = '$e');
      }
    }
  }

  @override
  void dispose() {
    _offscreenDisposeTimer?.cancel();
    _controller?.player.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_error != null) {
      return Container(
        height: 200,
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        child: Center(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: ErrorStateText('Video unavailable: $_error'),
          ),
        ),
      );
    }
    if (!_isInitialized || _controller == null) {
      if (!_started && widget.visible) _start();
      return Container(
        height: 200,
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        child: Center(
          child: _started
              ? const CircularProgressIndicator()
              : const Icon(Icons.movie_outlined, color: Colors.grey),
        ),
      );
    }

    final controller = _controller!;

    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: AspectRatio(
        aspectRatio: 16 / 9,
        child: Stack(
          alignment: Alignment.center,
          children: [
            ExcludeSemantics(
              child: Video(controller: controller, fit: BoxFit.contain),
            ),
            Positioned(
              right: 8,
              bottom: 8,
              child: StreamBuilder<bool>(
                stream: controller.player.stream.playing,
                initialData: controller.player.state.playing,
                builder: (context, snapshot) {
                  final isPlaying = snapshot.data ?? false;
                  return IconButton(
                    icon: Icon(
                      isPlaying ? Icons.pause : Icons.play_arrow,
                      color: Colors.white,
                    ),
                    onPressed: () {
                      if (isPlaying) {
                        controller.player.pause();
                      } else {
                        controller.player.play();
                      }
                    },
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
