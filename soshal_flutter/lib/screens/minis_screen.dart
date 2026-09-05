import 'dart:convert';
import 'dart:math' as math;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../widgets/mini_video_player.dart';
import '../services/feed_service.dart';
import '../services/media_service.dart';
import '../services/minis_service.dart';
import '../services/p2p_service.dart';
import '../services/session_service.dart';
import '../widgets/blob_image.dart';
import '../widgets/empty_state.dart';
import '../utils/format.dart';

/// Minis: mini video registry (kind-31020). Each mini is a video hosted
/// from device caches — the local chunk store first, then LAN peers, with
/// the original URL as fallback. Tapping plays the video in-app.
class MinisScreen extends StatefulWidget {
  /// Minis screen.
  const MinisScreen({super.key});

  @override
  State<MinisScreen> createState() => _MinisScreenState();
}

class _MinisScreenState extends State<MinisScreen>
    with SingleTickerProviderStateMixin {
  List<MiniItem> _minis = [];
  bool _loading = true;
  String _filterText = '';
  String? _filterResult;
  bool _rankOn = false;
  List<String> _ranked = [];
  bool _wasmRuntimeUnavailable = false;
  final Map<String, bool> _likedState = {};
  final Map<String, int> _likeDelta = {};

  late TabController _tabs;

  @override
  void initState() {
    super.initState();
    _tabs = TabController(length: 4, vsync: this);
    _tabs.addListener(_onTabChanged);
    _load();
    _loadSaved();
  }

  Future<void> _load() => _loadRecent();

  double _hotScore(MiniItem m) {
    final hours =
        ((DateTime.now().millisecondsSinceEpoch / 1000) - m.createdAt) / 3600;
    return m.reactions / math.pow(hours + 2, 1.5).toDouble();
  }

  void _seedLikeState() {
    for (final m in _minis) {
      _likedState[m.id] = m.liked;
      _likeDelta[m.id] = 0;
    }
  }

  Future<void> _loadRecent() async {
    setState(() => _loading = true);
    try {
      _minis = context.read<MinisService>().fetchMinis();
      _seedLikeState();
    } catch (e) {
      debugPrint('minis load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _loadTop() async {
    setState(() => _loading = true);
    try {
      _minis = context.read<MinisService>().fetchMinis();
      _minis.sort((a, b) => _hotScore(b).compareTo(_hotScore(a)));
      _seedLikeState();
    } catch (e) {
      debugPrint('minis load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _loadFollowing() async {
    setState(() => _loading = true);
    try {
      _minis = context.read<MinisService>().fetchMinis(audience: 'following');
      _seedLikeState();
    } catch (e) {
      debugPrint('minis load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _loadSaved() async {
    try {
      await context.read<MinisService>().fetchSavedMinis();
    } catch (e) {
      debugPrint('minis saved load: $e');
    }
  }

  void _onTabChanged() {
    if (_tabs.index == 0) _loadTop();
    if (_tabs.index == 1) _loadRecent();
    if (_tabs.index == 2) _loadFollowing();
    if (_tabs.index == 3) _loadSaved();
  }

  Future<void> _toggleSave(MiniItem mini) async {
    final service = context.read<MinisService>();
    final messenger = ScaffoldMessenger.of(context);
    if (service.isSaved(mini.id)) {
      await service.unsaveMini(mini.id);
      messenger.showSnackBar(
        const SnackBar(content: Text('Mini removed from Saved')),
      );
      return;
    }
    final hosted = await service.saveMini(
      mini,
      media: context.read<MediaService>(),
      p2p: context.read<P2pService>(),
    );
    messenger.showSnackBar(
      SnackBar(
        content: Text(hosted
            ? 'Mini saved — hosting on this device'
            : 'Mini saved — blob not yet available on this device'),
      ),
    );
  }

  Future<void> _toggleLike(MiniItem m) async {
    final me = context.read<SessionService>().activePubkey;
    if (me == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Sign in to react')),
      );
      return;
    }
    final current = _likedState[m.id] ?? m.liked;
    _likedState[m.id] = !current;
    _likeDelta[m.id] = (_likeDelta[m.id] ?? 0) + (!current ? 1 : -1);
    setState(() {});
    try {
      await context
          .read<FeedService>()
          .createReaction(m.id, current ? '-' : '+', me);
    } catch (_) {
      _likedState[m.id] = current;
      _likeDelta[m.id] = (_likeDelta[m.id] ?? 0) - (!current ? 1 : -1);
      setState(() {});
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Reaction error')),
        );
      }
    }
  }

  void _runFilter() {
    final text = _filterText.trim();
    if (text.isEmpty) return;
    final service = context.read<MinisService>();
    try {
      final result = service.runFilter(
        pluginId: 'content-filter',
        text: text,
        wasmBytesHex: '',
      );
      if (!mounted) return;
      setState(() {
        _wasmRuntimeUnavailable = service.wasmRuntimeUnavailable;
        _filterResult = _wasmRuntimeUnavailable ? null : result;
      });
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _wasmRuntimeUnavailable = true;
        _filterResult = null;
      });
    }
  }

  Future<void> _toggleRank(bool on) async {
    setState(() => _rankOn = on);
    if (!on) {
      setState(() => _ranked = []);
      return;
    }
    if (_minis.isEmpty) return;
    final service = context.read<MinisService>();
    final posts = _minis.map((m) => jsonEncode({'url': m.videoUrl})).toList();
    try {
      final ranked = service.rankFeed(
        pluginId: 'feed-ranker',
        postsJson: posts,
        wasmBytesHex: '',
      );
      if (!mounted) return;
      setState(() {
        _wasmRuntimeUnavailable = service.wasmRuntimeUnavailable;
        _ranked = ranked;
        if (_wasmRuntimeUnavailable) _rankOn = false;
      });
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _wasmRuntimeUnavailable = true;
        _ranked = [];
        _rankOn = false;
      });
    }
  }

  Future<void> _play(MiniItem mini) async {
    final url = await resolveMiniPlaybackUrl(
      mini,
      context.read<MediaService>(),
      context.read<P2pService>(),
    );
    if (!mounted) return;
    if (url == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
            content: Text('Mini unavailable — no device has this blob.')),
      );
      return;
    }
    if (mini.videoUrl.startsWith('http') && url == mini.videoUrl) {
      // Remote fallback: offer to open externally (no in-app webview).
      showModalBottomSheet<void>(
        context: context,
        builder: (sheetContext) => SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text('Mini',
                    style: Theme.of(sheetContext).textTheme.titleLarge),
                const SizedBox(height: 8),
                Text(
                  mini.videoUrl,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(sheetContext).textTheme.bodyMedium,
                ),
                const SizedBox(height: 16),
                FilledButton.icon(
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: mini.videoUrl));
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
      return;
    }
    await showDialog<void>(
      context: context,
      builder: (_) => MiniVideoPlayer(url),
    );
  }

  Future<void> _openUpload() async {
    final overlay = TextEditingController();
    var busy = false;
    var status = '';

    try {
      await showModalBottomSheet<void>(
        context: context,
        isScrollControlled: true,
        builder: (sheetContext) => StatefulBuilder(
          builder: (sbContext, setSheetState) => Padding(
            padding: EdgeInsets.only(
              left: 16,
              right: 16,
              top: 16,
              bottom: MediaQuery.of(sbContext).viewInsets.bottom + 16,
            ),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('New Mini',
                      style: Theme.of(sbContext).textTheme.titleLarge),
                  const SizedBox(height: 12),
                  Text(
                    'Pick a video file — it is hosted from device caches '
                    '(yours + devices that play it).',
                    style: Theme.of(sbContext).textTheme.bodySmall,
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: overlay,
                    decoration: const InputDecoration(
                      labelText: 'Caption (optional)',
                    ),
                  ),
                  const SizedBox(height: 16),
                  FilledButton(
                    onPressed: busy
                        ? null
                        : () async {
                            final picked = await FilePicker.pickFile(
                              type: FileType.video,
                            );
                            final path = picked?.path;
                            if (path == null || !sheetContext.mounted) return;
                            setSheetState(() {
                              busy = true;
                              status = 'Publishing…';
                            });
                            try {
                              final id = await sheetContext
                                  .read<MinisService>()
                                  .publishMini(
                                    mediaSource: path,
                                    textOverlay: overlay.text.trim().isEmpty
                                        ? null
                                        : overlay.text.trim(),
                                  );
                              if (!sheetContext.mounted) return;
                              Navigator.of(sheetContext).pop();
                              if (!mounted) return;
                              if (mounted) {
                                ScaffoldMessenger.of(context).showSnackBar(
                                  SnackBar(
                                    content:
                                        SelectableText('Mini published: $id'),
                                  ),
                                );
                              }
                              await _load();
                            } catch (e) {
                              if (sheetContext.mounted) {
                                setSheetState(
                                    () => status = 'Publish failed: $e');
                              }
                            }
                          },
                    child: const Text('Pick video & publish'),
                  ),
                  if (status.isNotEmpty) ...[
                    const SizedBox(height: 8),
                    Text(status,
                        style: TextStyle(
                          color: Theme.of(sbContext).colorScheme.error,
                        )),
                  ],
                ],
              ),
            ),
          ),
        ),
      );
    } finally {
      overlay.dispose();
    }
  }

  bool _reelsMode = true;
  final PageController _pageController = PageController();

  @override
  void dispose() {
    _tabs.dispose();
    _pageController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Minis'),
        actions: [
          IconButton(
            icon: Icon(
                _reelsMode ? Icons.view_list : Icons.video_collection_outlined),
            tooltip: _reelsMode ? 'Switch to list' : 'Switch to Reels feed',
            onPressed: () => setState(() => _reelsMode = !_reelsMode),
          ),
        ],
        bottom: TabBar(
          controller: _tabs,
          tabs: const [
            Tab(text: 'Top'),
            Tab(text: 'Recent'),
            Tab(text: 'Following'),
            Tab(text: 'Saved'),
          ],
        ),
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: _openUpload,
        tooltip: 'Publish mini',
        child: const Icon(Icons.add),
      ),
      body: TabBarView(
        controller: _tabs,
        children: [
          _buildForYou(),
          _buildForYou(),
          _buildForYou(),
          _buildSaved(),
        ],
      ),
    );
  }

  Widget _buildForYou() {
    final display = _rankOn && _ranked.isNotEmpty ? _ranked : _minis;
    return _loading
        ? const Center(child: CircularProgressIndicator())
        : _reelsMode && _minis.isNotEmpty
            ? _buildReelsFeed()
            : RefreshIndicator(
                onRefresh: _load,
                child: ListView(
                  padding: const EdgeInsets.all(16),
                  children: [
                    if (_wasmRuntimeUnavailable) ...[
                      const SizedBox(height: 8),
                      Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Icon(
                            Icons.info_outline,
                            size: 16,
                            color:
                                Theme.of(context).colorScheme.onSurfaceVariant,
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              'WASM runtime unavailable (roadmap): plugin '
                              'execution is simulated in this build.',
                              style: TextStyle(
                                fontSize: 12,
                                color: Theme.of(context)
                                    .colorScheme
                                    .onSurfaceVariant,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ],
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
                    Text('Minis',
                        style: Theme.of(context).textTheme.titleMedium),
                    const SizedBox(height: 8),
                    if (display.isEmpty)
                      EmptyState(
                        compact: true,
                        icon: Icons.video_library,
                        title: 'No minis yet',
                        body:
                            'Publish a mini video — it is hosted from device caches, not URL links.',
                      )
                    else if (_rankOn && _ranked.isNotEmpty)
                      for (final url in _ranked) ...[
                        ListTile(
                          leading: Icon(
                            Icons.video_library,
                            color: Theme.of(context).colorScheme.primary,
                          ),
                          title: Text(url,
                              maxLines: 1, overflow: TextOverflow.ellipsis),
                          subtitle: const Text('Mini video (ranked)'),
                        ),
                        const Divider(height: 1),
                      ]
                    else
                      for (var i = 0; i < display.length; i++) ...[
                        ListTile(
                          leading: _minis[i].thumbnail.isNotEmpty
                              ? ClipRRect(
                                  borderRadius: BorderRadius.circular(8),
                                  child: BlobImage(
                                    source: _minis[i].thumbnail,
                                    width: 48,
                                    height: 48,
                                    fit: BoxFit.cover,
                                    errorBuilder: (_) => Icon(
                                      Icons.video_library,
                                      color:
                                          Theme.of(context).colorScheme.primary,
                                    ),
                                  ),
                                )
                              : Icon(
                                  Icons.video_library,
                                  color: Theme.of(context).colorScheme.primary,
                                ),
                          title: Text(
                            _minis[i].textOverlay.isEmpty
                                ? (_minis[i].videoUrl.startsWith('blob://')
                                    ? 'Mini video'
                                    : _minis[i].videoUrl)
                                : _minis[i].textOverlay,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle: Text(
                            _minis[i].videoUrl.startsWith('blob://')
                                ? 'Mini video · hosted from device caches'
                                : 'Mini video · ${_minis[i].videoUrl}',
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          trailing: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              if (!_rankOn)
                                Builder(
                                  builder: (ctx) {
                                    final saved = ctx
                                        .watch<MinisService>()
                                        .isSaved(_minis[i].id);
                                    return IconButton(
                                      icon: Icon(
                                        saved
                                            ? Icons.bookmark
                                            : Icons.bookmark_border,
                                      ),
                                      tooltip: saved
                                          ? 'Unsave'
                                          : 'Save (and host on this device)',
                                      onPressed: () => _toggleSave(_minis[i]),
                                    );
                                  },
                                ),
                              IconButton(
                                icon: const Icon(Icons.play_circle_outline),
                                tooltip: 'Play',
                                onPressed: () => _play(_minis[i]),
                              ),
                            ],
                          ),
                          onTap: () => _play(_minis[i]),
                        ),
                        if (i < display.length - 1) const Divider(height: 1),
                      ],
                  ],
                ),
              );
  }

  Widget _buildSaved() {
    final saved = context.watch<MinisService>().savedMinis;
    return RefreshIndicator(
      onRefresh: _loadSaved,
      child: saved.isEmpty
          ? ListView(
              children: const [
                SizedBox(height: 120),
                EmptyState(
                  icon: Icons.bookmark_border,
                  title: 'No saved minis',
                  body:
                      'Tap the bookmark on a mini that you like — saving also '
                      'hosts its video from your device for other users.',
                ),
              ],
            )
          : ListView.separated(
              itemCount: saved.length,
              separatorBuilder: (_, __) => const Divider(height: 1),
              itemBuilder: (context, index) {
                final mini = saved[index];
                return ListTile(
                  leading: mini.thumbnail.isNotEmpty
                      ? ClipRRect(
                          borderRadius: BorderRadius.circular(8),
                          child: BlobImage(
                            source: mini.thumbnail,
                            width: 48,
                            height: 48,
                            fit: BoxFit.cover,
                            errorBuilder: (_) => Icon(
                              Icons.video_library,
                              color: Theme.of(context).colorScheme.primary,
                            ),
                          ),
                        )
                      : Icon(Icons.video_library,
                          color: Theme.of(context).colorScheme.primary),
                  title: Text(
                    mini.textOverlay.isEmpty
                        ? (mini.videoUrl.startsWith('blob://')
                            ? 'Mini video'
                            : mini.videoUrl)
                        : mini.textOverlay,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  subtitle: Text(
                    'Saved mini · ${relativeTime(mini.createdAt)}',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        icon: const Icon(Icons.bookmark),
                        tooltip: 'Unsave',
                        onPressed: () => _toggleSave(mini),
                      ),
                      IconButton(
                        icon: const Icon(Icons.play_circle_outline),
                        tooltip: 'Play',
                        onPressed: () => _play(mini),
                      ),
                    ],
                  ),
                  onTap: () => _play(mini),
                );
              },
            ),
    );
  }

  Widget _buildReelsFeed() {
    return PageView.builder(
      controller: _pageController,
      scrollDirection: Axis.vertical,
      itemCount: _minis.length,
      itemBuilder: (context, index) {
        final mini = _minis[index];
        final liked = _likedState[mini.id] ?? mini.liked;
        final count =
            (mini.reactions + (_likeDelta[mini.id] ?? 0)).clamp(0, 1 << 30);
        return Stack(
          fit: StackFit.expand,
          children: [
            // Background Video Thumbnail or Player placeholder
            mini.thumbnail.isNotEmpty
                ? BlobImage(
                    source: mini.thumbnail,
                    fit: BoxFit.cover,
                  )
                : Container(
                    color: Colors.black87,
                    child: Center(
                      child: Icon(
                        Icons.play_circle_fill,
                        size: 72,
                        color: Theme.of(context)
                            .colorScheme
                            .primary
                            .withAlpha(200),
                      ),
                    ),
                  ),
            // Gradient scrim
            const DecoratedBox(
              decoration: BoxDecoration(
                gradient: LinearGradient(
                  begin: Alignment.topCenter,
                  end: Alignment.bottomCenter,
                  colors: [Colors.black26, Colors.transparent, Colors.black87],
                  stops: [0.0, 0.6, 1.0],
                ),
              ),
            ),
            // Center Play Tap target
            Positioned.fill(
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => _play(mini),
                child: const SizedBox.expand(),
              ),
            ),
            // Right-side Action Rail
            Positioned(
              right: 16,
              bottom: 80,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    icon: Icon(
                      liked ? Icons.favorite : Icons.favorite_border,
                      color: liked ? Colors.pinkAccent : Colors.white,
                      size: 30,
                    ),
                    tooltip: liked ? 'Unlike' : 'Like',
                    onPressed: () => _toggleLike(mini),
                  ),
                  Text('$count',
                      style: const TextStyle(
                          color: Colors.white, fontSize: 11)),
                  const SizedBox(height: 16),
                  IconButton(
                    icon: const Icon(Icons.chat_bubble_outline,
                        color: Colors.white, size: 28),
                    tooltip: 'Comments',
                    onPressed: () => _play(mini),
                  ),
                  const Text('Comments',
                      style: TextStyle(color: Colors.white, fontSize: 11)),
                  const SizedBox(height: 16),
                  IconButton(
                    icon:
                        const Icon(Icons.share, color: Colors.white, size: 28),
                    tooltip: 'Share',
                    onPressed: () {
                      Clipboard.setData(ClipboardData(text: mini.videoUrl));
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Reels link copied!')),
                      );
                    },
                  ),
                  const Text('Share',
                      style: TextStyle(color: Colors.white, fontSize: 11)),
                  const SizedBox(height: 16),
                  Builder(
                    builder: (ctx) {
                      final saved = ctx.watch<MinisService>().isSaved(mini.id);
                      return IconButton(
                        icon: Icon(
                          saved ? Icons.bookmark : Icons.bookmark_outline,
                          color: Colors.white,
                          size: 28,
                        ),
                        tooltip:
                            saved ? 'Unsave' : 'Save (and host on this device)',
                        onPressed: () => _toggleSave(mini),
                      );
                    },
                  ),
                  const Text('Save',
                      style: TextStyle(color: Colors.white, fontSize: 11)),
                  const SizedBox(height: 16),
                  // Rotating Audio Disc Thumbnail
                  GestureDetector(
                    onTap: () {
                      ScaffoldMessenger.of(context).showSnackBar(
                        SnackBar(
                            content: Text(
                                'Audio: ${mini.videoUrl.split('/').last}')),
                      );
                    },
                    child: Container(
                      width: 36,
                      height: 36,
                      decoration: BoxDecoration(
                        color: Colors.grey.shade900,
                        shape: BoxShape.circle,
                        border: Border.all(color: Colors.white54, width: 2),
                      ),
                      child: const Icon(Icons.album,
                          color: Colors.white, size: 22),
                    ),
                  ),
                ],
              ),
            ),
            // Bottom Creator & Sound Info Overlay
            Positioned(
              left: 16,
              right: 80,
              bottom: 24,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    mini.textOverlay.isNotEmpty
                        ? mini.textOverlay
                        : 'Reels Video',
                    style: const TextStyle(
                      color: Colors.white,
                      fontSize: 16,
                      fontWeight: FontWeight.bold,
                    ),
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                  const SizedBox(height: 8),
                  Row(
                    children: [
                      const Icon(Icons.music_note,
                          color: Colors.white70, size: 16),
                      const SizedBox(width: 4),
                      Expanded(
                        child: Text(
                          'Original Audio · ${mini.videoUrl.split('/').last}',
                          style: const TextStyle(
                              color: Colors.white70, fontSize: 12),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }
}
