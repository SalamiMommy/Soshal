import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../services/music_service.dart';
import '../services/shell_service.dart';
import '../utils/format.dart';
import '../widgets/error_state_text.dart';

/// Musicloud: track list, publish form (FAB), and a detail view with
/// comments and share-to-feed. Playback goes through the global audio bar.
class MusicloudScreen extends StatefulWidget {
  /// Musicloud screen.
  const MusicloudScreen({super.key});

  @override
  State<MusicloudScreen> createState() => _MusicloudScreenState();
}

class _MusicloudScreenState extends State<MusicloudScreen> {
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      await context.read<MusicService>().fetchTracks();
    } catch (e) {
      debugPrint('musicloud load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _openUpload() async {
    final url = TextEditingController();
    final title = TextEditingController();
    final hashtags = TextEditingController();
    var busy = false;
    var status = '';

    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (sheetContext) => StatefulBuilder(
        builder: (context, setSheetState) => Padding(
          padding: EdgeInsets.only(
            left: 16,
            right: 16,
            top: 16,
            bottom: MediaQuery.of(context).viewInsets.bottom + 16,
          ),
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text('New Track',
                    style: Theme.of(context).textTheme.titleLarge),
                const SizedBox(height: 12),
                TextField(
                  controller: url,
                  keyboardType: TextInputType.url,
                  decoration: const InputDecoration(
                    labelText: 'Audio URL (https mp3/ogg…) *',
                  ),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: title,
                  decoration: const InputDecoration(labelText: 'Title'),
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: hashtags,
                  decoration: const InputDecoration(
                    labelText: 'Hashtags (comma-separated)',
                  ),
                ),
                const SizedBox(height: 16),
                FilledButton(
                  onPressed: busy
                      ? null
                      : () async {
                          final audioUrl = url.text.trim();
                          if (audioUrl.isEmpty) {
                            setSheetState(
                                () => status = 'Audio URL is required.');
                            return;
                          }
                          setSheetState(() {
                            busy = true;
                            status = 'Publishing…';
                          });
                          try {
                            final id =
                                await context.read<MusicService>().publishTrack(
                                      audioUrl: audioUrl,
                                      title: title.text.trim().isEmpty
                                          ? null
                                          : title.text.trim(),
                                      thumbnail: null,
                                      hashtags: hashtags.text
                                          .split(',')
                                          .map((h) => h.trim())
                                          .where((h) => h.isNotEmpty)
                                          .toList(),
                                    );
                            if (!sheetContext.mounted) return;
                            Navigator.of(sheetContext).pop();
                            if (!context.mounted) return;
                            ScaffoldMessenger.of(context).showSnackBar(
                              SnackBar(
                                content: SelectableText('Track published: $id'),
                              ),
                            );
                            await _load();
                          } catch (e) {
                            if (mounted) {
                              setSheetState(
                                  () => status = 'Publish failed: $e');
                            }
                          }
                        },
                  child: const Text('Publish'),
                ),
                if (status.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Text(status,
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      )),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  void _play(MusicTrack track) {
    context.read<ShellService>().playAudio(track.audioUrl, track.title);
    Clipboard.setData(ClipboardData(text: track.audioUrl));
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: SelectableText(
            'Playing — ${track.title} · audio URL copied to clipboard'),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final tracks = context.watch<MusicService>().tracks;
    final error = context.watch<MusicService>().lastError;
    return Scaffold(
      appBar: AppBar(title: const Text('Musicloud')),
      floatingActionButton: FloatingActionButton(
        onPressed: _openUpload,
        tooltip: 'Publish track',
        child: const Icon(Icons.add),
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : RefreshIndicator(
              onRefresh: _load,
              child: tracks.isEmpty
                  ? ListView(
                      children: [
                        const SizedBox(height: 120),
                        Center(
                          child: Padding(
                            padding: const EdgeInsets.all(24),
                            child: Column(
                              children: [
                                Icon(
                                  Icons.music_note,
                                  size: 56,
                                  color: Theme.of(context).colorScheme.outline,
                                ),
                                const SizedBox(height: 16),
                                Text('No songs found',
                                    style:
                                        Theme.of(context).textTheme.titleLarge),
                                const SizedBox(height: 8),
                                Text(
                                  'Be the first to publish an audio track on Musicloud — tap +.',
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
                      itemCount: tracks.length,
                      separatorBuilder: (_, __) => const Divider(height: 1),
                      itemBuilder: (context, index) {
                        final track = tracks[index];
                        return ListTile(
                          leading: track.thumbnail.isNotEmpty
                              ? ClipRRect(
                                  borderRadius: BorderRadius.circular(8),
                                  child: Image.network(
                                    track.thumbnail,
                                    width: 48,
                                    cacheWidth: 160,
                                    height: 48,
                                    fit: BoxFit.cover,
                                    errorBuilder: (_, __, ___) =>
                                        _trackIcon(context),
                                  ),
                                )
                              : _trackIcon(context),
                          title: Text(
                            track.title.isEmpty
                                ? 'Untitled track'
                                : track.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle: Text(
                            [
                              shortPubkey(track.pubkey),
                              relativeTime(track.createdAt),
                              if (track.hashtags.isNotEmpty)
                                track.hashtags.map((h) => '#$h').join(' '),
                            ].join(' · '),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                          ),
                          trailing: IconButton(
                            icon: const Icon(Icons.play_circle_outline),
                            tooltip: 'Play',
                            onPressed: () => _play(track),
                          ),
                          onTap: () {
                            Navigator.of(context).push(
                              MaterialPageRoute<void>(
                                builder: (_) => _TrackDetailScreen(track),
                              ),
                            );
                          },
                        );
                      },
                    ),
            ),
      bottomNavigationBar: error != null && error.isNotEmpty
          ? Material(
              color: Theme.of(context).colorScheme.errorContainer,
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: ErrorStateText('Error: $error'),
              ),
            )
          : null,
    );
  }

  Widget _trackIcon(BuildContext context) => Container(
        width: 48,
        height: 48,
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(8),
        ),
        child: Icon(Icons.music_note,
            color: Theme.of(context).colorScheme.primary),
      );
}

/// Track detail: share-to-feed form + comment thread.
class _TrackDetailScreen extends StatefulWidget {
  final MusicTrack track;

  /// Track detail screen.
  const _TrackDetailScreen(this.track);

  @override
  State<_TrackDetailScreen> createState() => _TrackDetailScreenState();
}

class _TrackDetailScreenState extends State<_TrackDetailScreen> {
  final _shareCtrl = TextEditingController();
  final _commentCtrl = TextEditingController();
  List<TrackComment> _comments = [];
  bool _commentsLoading = false;
  bool _shareBusy = false;
  bool _commentBusy = false;
  String? _shareStatus;
  String? _commentStatus;

  MusicTrack get _track => widget.track;

  String get _trackD => _track.d.isNotEmpty ? _track.d : _track.id;

  @override
  void initState() {
    super.initState();
    _loadComments();
  }

  @override
  void dispose() {
    _shareCtrl.dispose();
    _commentCtrl.dispose();
    super.dispose();
  }

  Future<void> _loadComments() async {
    setState(() => _commentsLoading = true);
    try {
      final comments = await context.read<MusicService>().fetchComments(
            trackPubkey: _track.pubkey,
            trackD: _trackD,
          );
      if (mounted) setState(() => _comments = comments);
    } catch (e) {
      debugPrint('comments load: $e');
    }
    if (mounted) setState(() => _commentsLoading = false);
  }

  Future<void> _share() async {
    final message = _shareCtrl.text.trim();
    if (message.isEmpty) {
      setState(() => _shareStatus = 'Write a message first.');
      return;
    }
    setState(() {
      _shareBusy = true;
      _shareStatus = null;
    });
    try {
      final id = await context.read<MusicService>().shareToFeed(
            trackId: _track.id,
            trackPubkey: _track.pubkey,
            message: message,
            hashtags: _track.hashtags,
          );
      if (!mounted) return;
      setState(() {
        _shareCtrl.clear();
        _shareStatus = 'Shared to feed: $id';
      });
    } catch (e) {
      if (mounted) setState(() => _shareStatus = 'Share failed: $e');
    } finally {
      if (mounted) setState(() => _shareBusy = false);
    }
  }

  Future<void> _postComment() async {
    final content = _commentCtrl.text.trim();
    if (content.isEmpty) return;
    setState(() {
      _commentBusy = true;
      _commentStatus = null;
    });
    try {
      await context.read<MusicService>().comment(
            trackPubkey: _track.pubkey,
            trackD: _trackD,
            content: content,
          );
      if (!mounted) return;
      _commentCtrl.clear();
      await _loadComments();
      setState(() => _commentStatus = 'Comment posted.');
    } catch (e) {
      if (mounted) setState(() => _commentStatus = 'Failed: $e');
    } finally {
      if (mounted) setState(() => _commentBusy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar:
          AppBar(title: Text(_track.title.isEmpty ? 'Track' : _track.title)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(shortPubkey(_track.pubkey),
                        style: Theme.of(context).textTheme.labelMedium),
                    const SizedBox(height: 4),
                    Text(relativeTime(_track.createdAt),
                        style: Theme.of(context).textTheme.bodySmall),
                    if (_track.hashtags.isNotEmpty) ...[
                      const SizedBox(height: 8),
                      Wrap(
                        spacing: 8,
                        children: _track.hashtags
                            .map((h) => Chip(
                                  label: Text('#$h'),
                                  visualDensity: VisualDensity.compact,
                                ))
                            .toList(),
                      ),
                    ],
                  ],
                ),
              ),
              FilledButton.icon(
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: _track.audioUrl));
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: Text('Audio URL copied to clipboard')),
                  );
                },
                icon: const Icon(Icons.copy),
                label: const Text('Copy URL'),
              ),
            ],
          ),
          const SizedBox(height: 24),
          Text('Share to Feed', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          TextField(
            controller: _shareCtrl,
            maxLines: 3,
            decoration: const InputDecoration(
              hintText: 'Write a message…',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 8),
          Align(
            alignment: Alignment.centerRight,
            child: FilledButton.icon(
              onPressed: _shareBusy ? null : _share,
              icon: const Icon(Icons.send),
              label: Text(_shareBusy ? 'Sharing…' : 'Share'),
            ),
          ),
          if (_shareStatus != null) ...[
            const SizedBox(height: 8),
            Text(_shareStatus!,
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                )),
          ],
          const Divider(height: 40),
          Row(
            children: [
              Expanded(
                child: Text('Comments',
                    style: Theme.of(context).textTheme.titleMedium),
              ),
              if (_commentsLoading)
                const SizedBox(
                  width: 16,
                  height: 16,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              IconButton(
                icon: const Icon(Icons.refresh),
                tooltip: 'Refresh',
                onPressed: _loadComments,
              ),
            ],
          ),
          if (_comments.isEmpty && !_commentsLoading)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 12),
              child: Text(
                'No comments yet. Be the first to say something.',
                style: TextStyle(
                    color: Theme.of(context).colorScheme.onSurfaceVariant),
              ),
            ),
          ..._comments.map(
            (c) => Padding(
              padding: const EdgeInsets.symmetric(vertical: 6),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    '${shortPubkey(c.pubkey)} · ${relativeTime(c.createdAt)}',
                    style: Theme.of(context).textTheme.labelSmall,
                  ),
                  const SizedBox(height: 2),
                  Text(c.content),
                ],
              ),
            ),
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _commentCtrl,
                  onSubmitted: (_) => _postComment(),
                  decoration: const InputDecoration(
                    hintText: 'Comment on track…',
                    border: OutlineInputBorder(),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              IconButton.filled(
                icon: const Icon(Icons.send),
                tooltip: 'Post',
                onPressed: _commentBusy ? null : _postComment,
              ),
            ],
          ),
          if (_commentStatus != null) ...[
            const SizedBox(height: 8),
            Text(_commentStatus!,
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                )),
          ],
        ],
      ),
    );
  }
}
