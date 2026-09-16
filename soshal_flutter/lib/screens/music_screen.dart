import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/friends_service.dart';
import '../services/media_service.dart';
import '../services/music_service.dart';
import '../services/p2p_service.dart';
import '../services/shell_service.dart';
import '../utils/format.dart';
import '../widgets/audience_filter_dropdown.dart';
import '../widgets/blob_image.dart';
import '../widgets/empty_state.dart';
import '../widgets/error_state_text.dart';

/// Musicloud: track list, publish form (FAB), and a detail view with
/// comments and share-to-feed. Playback goes through the global audio bar.
class MusicloudScreen extends StatefulWidget {
  /// Musicloud screen.
  const MusicloudScreen({super.key});

  @override
  State<MusicloudScreen> createState() => _MusicloudScreenState();
}

class _MusicloudScreenState extends State<MusicloudScreen>
    with SingleTickerProviderStateMixin {
  bool _loading = true;
  AudienceFilter _audienceFilter = AudienceFilter.all;
  late TabController _tabs;
  final Map<String, List<MusicTrack>> _playlistTracks = {};
  final Set<String> _loadingPlaylists = {};

  @override
  void initState() {
    super.initState();
    _tabs = TabController(length: 3, vsync: this);
    _tabs.addListener(_onTabChanged);
    final myPk = context.activePubkeyOrNull;
    if (myPk != null) {
      context.friendsServiceReadOrNull?.loadAudienceGraph(myPk);
    }
    _load();
    _warm();
  }

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  void _onTabChanged() {
    if (_tabs.index == 1) {
      _loadSaved();
    } else if (_tabs.index == 2) {
      _loadPlaylists();
    }
  }

  Future<void> _warm() async {
    final service = context.read<MusicService>();
    try {
      await service.fetchSavedTracks();
    } catch (e) {
      debugPrint('musicloud saved warm: $e');
    }
    try {
      await service.fetchPlaylists();
    } catch (e) {
      debugPrint('musicloud playlists warm: $e');
    }
  }

  Future<void> _loadSaved() async {
    try {
      await context.read<MusicService>().fetchSavedTracks();
    } catch (e) {
      debugPrint('musicloud saved load: $e');
    }
  }

  Future<void> _loadPlaylists() async {
    try {
      await context.read<MusicService>().fetchPlaylists();
    } catch (e) {
      debugPrint('musicloud playlists load: $e');
    }
  }

  Future<void> _toggleSaveTrack(MusicTrack track) async {
    final service = context.read<MusicService>();
    final messenger = ScaffoldMessenger.of(context);
    if (service.isTrackSaved(track.id)) {
      await service.unsaveTrack(track.id);
      messenger.showSnackBar(
        const SnackBar(content: Text('Track removed from Saved')),
      );
      return;
    }
    final hosted = await service.saveTrack(
      track,
      media: context.read<MediaService>(),
      p2p: context.read<P2pService>(),
    );
    messenger.showSnackBar(
      SnackBar(
        content: Text(hosted
            ? 'Track saved — hosting on this device'
            : 'Track saved — audio not yet available on this device'),
      ),
    );
  }

  Future<void> _togglePlaylist(String playlistId) async {
    if (_playlistTracks.containsKey(playlistId)) {
      setState(() => _playlistTracks.remove(playlistId));
      return;
    }
    setState(() => _loadingPlaylists.add(playlistId));
    try {
      final tracks =
          await context.read<MusicService>().fetchPlaylistTracks(playlistId);
      if (mounted) setState(() => _playlistTracks[playlistId] = tracks);
    } catch (e) {
      debugPrint('playlist tracks load: $e');
    }
    if (mounted) setState(() => _loadingPlaylists.remove(playlistId));
  }

  Future<void> _removeFromPlaylist(String playlistId, MusicTrack track) async {
    final service = context.read<MusicService>();
    try {
      await service.removeFromPlaylist(playlistId, track.id);
      await service.fetchPlaylists();
      final current = _playlistTracks[playlistId];
      if (current != null) {
        setState(() => _playlistTracks[playlistId] =
            current.where((t) => t.id != track.id).toList());
      }
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Track removed from playlist')),
        );
      }
    } catch (e) {
      debugPrint('playlist remove: $e');
    }
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final myPk = context.activePubkeyOrNull;
      if (myPk != null) {
        context.friendsServiceReadOrNull?.loadAudienceGraph(myPk);
      }
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
    String? pickedPath;
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
                  Text('New Track',
                      style: Theme.of(sbContext).textTheme.titleLarge),
                  const SizedBox(height: 12),
                  FilledButton.tonalIcon(
                    onPressed: () async {
                      final picked = await FilePicker.pickFile(
                        type: FileType.audio,
                      );
                      final path = picked?.path;
                      if (path == null) return;
                      setSheetState(() {
                        pickedPath = path;
                        status = '';
                      });
                    },
                    icon: const Icon(Icons.audio_file),
                    label: Text(pickedPath == null
                        ? 'Pick audio file (hosted from device caches)'
                        : 'Picked: ${pickedPath!.split('/').last}'),
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: url,
                    keyboardType: TextInputType.url,
                    decoration: const InputDecoration(
                      labelText:
                          '…or audio URL (https mp3/ogg…) — fallback when no device has the blob',
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
                            final mediaSource = pickedPath ?? url.text.trim();
                            if (mediaSource.isEmpty) {
                              setSheetState(() => status =
                                  'Pick an audio file or enter an audio URL.');
                              return;
                            }
                            setSheetState(() {
                              busy = true;
                              status = 'Publishing…';
                            });
                            try {
                              final id = await context
                                  .read<MusicService>()
                                  .publishTrack(
                                    mediaSource: mediaSource,
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
                              if (!mounted) return;
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(
                                  content:
                                      SelectableText('Track published: $id'),
                                ),
                              );
                              await _load();
                            } catch (e) {
                              if (sheetContext.mounted) {
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
      url.dispose();
      title.dispose();
      hashtags.dispose();
    }
  }

  Future<void> _play(MusicTrack track) async {
    final shell = context.read<ShellService>();
    final url = await resolveTrackPlaybackUrl(
      track,
      context.read<MediaService>(),
      context.read<P2pService>(),
    );
    if (!mounted) return;
    if (url == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
            content: Text('Audio unavailable — no device has this blob.')),
      );
      return;
    }
    final ok = await shell.playAudio(url, track.title);
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      ok
          ? SnackBar(
              content: SelectableText('Playing — ${track.title}'),
            )
          : const SnackBar(
              content: Text(
                  'Playback failed — check the audio output and try again.'),
            ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Musicloud'),
        actions: [
          AudienceFilterDropdown(
            value: _audienceFilter,
            onChanged: (filter) => setState(() => _audienceFilter = filter),
          ),
        ],
        bottom: TabBar(
          controller: _tabs,
          tabs: const [
            Tab(text: 'Browse'),
            Tab(text: 'Saved'),
            Tab(text: 'Playlists'),
          ],
        ),
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: _openUpload,
        tooltip: 'Publish track',
        child: const Icon(Icons.add),
      ),
      body: TabBarView(
        controller: _tabs,
        children: [
          _buildBrowse(),
          _buildSaved(),
          _buildPlaylists(),
        ],
      ),
      bottomNavigationBar: Builder(
        builder: (ctx) {
          final error = ctx.select((MusicService s) => s.lastError);
          if (error == null || error.isEmpty) return const SizedBox.shrink();
          return Material(
            color: Theme.of(ctx).colorScheme.errorContainer,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: ErrorStateText('Error: $error'),
            ),
          );
        },
      ),
    );
  }

  Widget _buildBrowse() {
    final rawTracks = context.select((MusicService s) => s.tracks);
    final friendsService = context.friendsServiceOrNull;
    final myPk = context.activePubkeyOrNull;
    final tracks = friendsService != null
        ? friendsService.filterList(
            rawTracks,
            _audienceFilter,
            (t) => t.pubkey,
            myPubkey: myPk,
          )
        : rawTracks;
    return _loading
        ? const Center(child: CircularProgressIndicator())
        : RefreshIndicator(
            onRefresh: _load,
            child: tracks.isEmpty
                ? ListView(
                    children: [
                      const SizedBox(height: 120),
                      EmptyState(
                        icon: Icons.music_note,
                        title: _audienceFilter == AudienceFilter.all
                            ? 'No songs found'
                            : 'No songs match ${_audienceFilter.label}',
                        body: _audienceFilter == AudienceFilter.all
                            ? 'Be the first to publish an audio track on Musicloud — tap +.'
                            : null,
                      ),
                    ],
                  )
                : ListView.separated(
                    itemCount: tracks.length,
                    separatorBuilder: (_, __) => const Divider(height: 1),
                    itemBuilder: (context, index) => _trackTile(tracks[index]),
                  ),
          );
  }

  Widget _buildSaved() {
    final rawSaved = context.select((MusicService s) => s.savedTracks);
    final friendsService = context.friendsServiceOrNull;
    final myPk = context.activePubkeyOrNull;
    final saved = friendsService != null
        ? friendsService.filterList(
            rawSaved,
            _audienceFilter,
            (t) => t.pubkey,
            myPubkey: myPk,
          )
        : rawSaved;
    return RefreshIndicator(
      onRefresh: _loadSaved,
      child: saved.isEmpty
          ? ListView(
              children: [
                const SizedBox(height: 120),
                EmptyState(
                  icon: Icons.bookmark_border,
                  title: _audienceFilter == AudienceFilter.all
                      ? 'No saved tracks'
                      : 'No saved tracks match ${_audienceFilter.label}',
                  body: _audienceFilter == AudienceFilter.all
                      ? 'Tap the bookmark on a song that you like — saving also '
                          'hosts its audio from your device for other users.'
                      : null,
                ),
              ],
            )
          : ListView.separated(
              itemCount: saved.length,
              separatorBuilder: (_, __) => const Divider(height: 1),
              itemBuilder: (context, index) => _trackTile(saved[index]),
            ),
    );
  }

  Widget _buildPlaylists() {
    final playlists = context.select((MusicService s) => s.playlists);
    return RefreshIndicator(
      onRefresh: _loadPlaylists,
      child: ListView(
        children: [
          Padding(
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                Expanded(
                  child: Text('My playlists',
                      style: Theme.of(context).textTheme.titleMedium),
                ),
                IconButton(
                  icon: const Icon(Icons.add),
                  tooltip: 'New playlist',
                  onPressed: _promptCreatePlaylist,
                ),
              ],
            ),
          ),
          if (playlists.isEmpty)
            const Padding(
              padding: EdgeInsets.only(top: 120),
              child: EmptyState(
                icon: Icons.queue_music,
                title: 'No playlists yet',
                body: 'Create a playlist to organise songs you love. '
                    'Use “Add to playlist” on any track.',
              ),
            )
          else
            for (final playlist in playlists) _playlistTile(playlist),
          const SizedBox(height: 24),
        ],
      ),
    );
  }

  Future<void> _promptCreatePlaylist() async {
    final result = await promptCreatePlaylistDialog(context);
    if (result == null || !mounted) return;
    try {
      await context.read<MusicService>().createPlaylist(
            title: result.name,
            isPrivate: result.isPrivate,
          );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Playlist “${result.name}” created')),
        );
      }
    } catch (e) {
      debugPrint('playlist create: $e');
    }
  }

  Widget _playlistTile(MusicPlaylist playlist) {
    final tracks = _playlistTracks[playlist.id];
    final loading = _loadingPlaylists.contains(playlist.id);
    return ExpansionTile(
      key: PageStorageKey(playlist.id),
      initiallyExpanded: _playlistTracks.containsKey(playlist.id),
      leading: const Icon(Icons.queue_music),
      title: Text(playlist.title, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        '${playlist.trackCount} tracks · '
        '${playlist.isPrivate ? 'Private' : 'Public'}',
      ),
      onExpansionChanged: (expanded) {
        if (expanded) _togglePlaylist(playlist.id);
      },
      trailing: const Icon(Icons.expand_more),
      children: loading
          ? const [
              Padding(
                padding: EdgeInsets.all(16),
                child: Center(
                  child: SizedBox(
                    width: 20,
                    height: 20,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                ),
              ),
            ]
          : (tracks ?? const <MusicTrack>[]).isEmpty
              ? const [
                  ListTile(
                    title: Text('No tracks in this playlist yet.'),
                  ),
                ]
              : [
                  for (final track in tracks ?? const <MusicTrack>[])
                    _playlistTrackTile(playlist, track),
                ],
    );
  }

  Widget _playlistTrackTile(MusicPlaylist playlist, MusicTrack track) {
    return ListTile(
      leading: const Icon(Icons.music_note),
      title: Text(
        track.title.isEmpty ? 'Untitled track' : track.title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      subtitle: Text(
        shortPubkey(track.pubkey),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            icon: const Icon(Icons.remove_circle_outline),
            tooltip: 'Remove from playlist',
            onPressed: () => _removeFromPlaylist(playlist.id, track),
          ),
          IconButton(
            icon: const Icon(Icons.play_circle_outline),
            tooltip: 'Play',
            onPressed: () => _play(track),
          ),
        ],
      ),
      onTap: () => context.push('/music/track', extra: track),
    );
  }

  Widget _trackTile(MusicTrack track) {
    return ListTile(
      leading: track.thumbnail.isNotEmpty
          ? ClipRRect(
              borderRadius: BorderRadius.circular(8),
              child: BlobImage(
                source: track.thumbnail,
                width: 48,
                height: 48,
                fit: BoxFit.cover,
                errorBuilder: (_) => _trackIcon(context),
              ),
            )
          : _trackIcon(context),
      title: Text(
        track.title.isEmpty ? 'Untitled track' : track.title,
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
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Builder(
            builder: (ctx) {
              final saved = ctx.watch<MusicService>().isTrackSaved(track.id);
              return IconButton(
                icon: Icon(saved ? Icons.bookmark : Icons.bookmark_border),
                tooltip: saved ? 'Unsave' : 'Save (and host on this device)',
                onPressed: () => _toggleSaveTrack(track),
              );
            },
          ),
          IconButton(
            icon: const Icon(Icons.play_circle_outline),
            tooltip: 'Play',
            onPressed: () => _play(track),
          ),
        ],
      ),
      onTap: () {
        context.push('/music/track', extra: track);
      },
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

/// Dialog prompting for a playlist name + privacy. Returns null when
/// cancelled.
Future<({String name, bool isPrivate})?> promptCreatePlaylistDialog(
  BuildContext context,
) async {
  final controller = TextEditingController();
  var isPrivate = true;
  final id = await showDialog<String>(
    context: context,
    builder: (dialogContext) => StatefulBuilder(
      builder: (dialogContext, setDialogState) => AlertDialog(
        title: const Text('New playlist'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: controller,
              autofocus: true,
              decoration: const InputDecoration(
                labelText: 'Playlist name',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 12),
            SwitchListTile(
              title: const Text('Private'),
              subtitle: const Text('Private playlists are only visible to you'),
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: isPrivate,
              onChanged: (v) => setDialogState(() => isPrivate = v),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () {
              final title = controller.text.trim();
              if (title.isEmpty) return;
              Navigator.of(dialogContext).pop('$title\u0000$isPrivate');
            },
            child: const Text('Create'),
          ),
        ],
      ),
    ),
  );
  controller.dispose();
  if (id == null) return null;
  return (
    name: id.split('\u0000').first,
    isPrivate: id.split('\u0000').last == 'true'
  );
}

/// Bottom sheet to pick a playlist (or create one) and add [track] to it.
Future<void> showAddToPlaylistSheet(
  BuildContext context,
  MusicService service,
  MusicTrack track,
) async {
  final playlists = await service.fetchPlaylists();
  if (!context.mounted) return;
  await showModalBottomSheet<void>(
    context: context,
    builder: (sheetContext) => SafeArea(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.all(16),
            child: Text('Add to playlist',
                style: Theme.of(sheetContext).textTheme.titleLarge),
          ),
          Flexible(
            child: ListView(
              shrinkWrap: true,
              children: [
                if (playlists.isEmpty)
                  const Padding(
                    padding: EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                    child: Text('No playlists yet — create one below.'),
                  ),
                for (final playlist in playlists)
                  ListTile(
                    leading: const Icon(Icons.queue_music),
                    title: Text(playlist.title,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text('${playlist.trackCount} tracks'),
                    onTap: () async {
                      final ok =
                          await service.addToPlaylist(playlist.id, track);
                      if (sheetContext.mounted) {
                        Navigator.of(sheetContext).pop();
                      }
                      if (!context.mounted) return;
                      ScaffoldMessenger.of(context).showSnackBar(
                        SnackBar(
                          content: Text(ok
                              ? 'Added to “${playlist.title}”'
                              : 'Already in “${playlist.title}”'),
                        ),
                      );
                    },
                  ),
                const Divider(),
                ListTile(
                  leading: const Icon(Icons.add),
                  title: const Text('New playlist…'),
                  onTap: () async {
                    final result =
                        await promptCreatePlaylistDialog(sheetContext);
                    if (result == null || !sheetContext.mounted) return;
                    final id = await service.createPlaylist(
                      title: result.name,
                      isPrivate: result.isPrivate,
                    );
                    final ok = await service.addToPlaylist(id, track);
                    if (sheetContext.mounted) {
                      Navigator.of(sheetContext).pop();
                    }
                    if (!context.mounted) return;
                    ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(
                        content: Text(ok
                            ? 'Added to “${result.name}”'
                            : 'Already in “${result.name}”'),
                      ),
                    );
                  },
                ),
              ],
            ),
          ),
        ],
      ),
    ),
  );
}

/// Track detail: share-to-feed form + comment thread.
class TrackDetailScreen extends StatefulWidget {
  final MusicTrack track;

  /// Track detail screen.
  const TrackDetailScreen(this.track, {super.key});

  @override
  State<TrackDetailScreen> createState() => TrackDetailScreenState();
}

class TrackDetailScreenState extends State<TrackDetailScreen> {
  final _shareCtrl = TextEditingController();
  final _commentCtrl = TextEditingController();
  List<TrackComment> _comments = [];
  bool _commentsLoading = false;
  bool _shareBusy = false;
  bool _commentBusy = false;
  String? _shareStatus;
  double _playbackPosition = 14.5;
  final double _trackDuration = 184.0;

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

  Future<void> _toggleSaveTrack() async {
    final service = context.read<MusicService>();
    final messenger = ScaffoldMessenger.of(context);
    if (service.isTrackSaved(_track.id)) {
      await service.unsaveTrack(_track.id);
      messenger.showSnackBar(
        const SnackBar(content: Text('Track removed from Saved')),
      );
      return;
    }
    final hosted = await service.saveTrack(
      _track,
      media: context.read<MediaService>(),
      p2p: context.read<P2pService>(),
    );
    messenger.showSnackBar(
      SnackBar(
        content: Text(hosted
            ? 'Track saved — hosting on this device'
            : 'Track saved — audio not yet available on this device'),
      ),
    );
  }

  void _addToPlaylist() {
    showAddToPlaylistSheet(
      context,
      context.read<MusicService>(),
      _track,
    );
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
            trackD: _track.d,
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

  String? _commentStatus;

  Future<void> _postComment() async {
    final raw = _commentCtrl.text.trim();
    if (raw.isEmpty) return;
    final timePrefix = '[${_playbackPosition.toStringAsFixed(0)}s] ';
    final content = '$timePrefix$raw';
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
      if (!mounted) return;
      setState(() => _commentStatus =
          'Comment posted at ${_playbackPosition.toStringAsFixed(0)}s.');
    } catch (e) {
      if (mounted) setState(() => _commentStatus = 'Failed: $e');
    } finally {
      if (mounted) setState(() => _commentBusy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final trackPlaying = context.select<ShellService, bool>(
        (s) => s.audioPlaying && s.audioTitle == _track.title);
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
              Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Builder(
                    builder: (ctx) {
                      final saved =
                          ctx.watch<MusicService>().isTrackSaved(_track.id);
                      return FilledButton.tonalIcon(
                        onPressed: _toggleSaveTrack,
                        icon: Icon(
                            saved ? Icons.bookmark : Icons.bookmark_border),
                        label: Text(saved ? 'Saved' : 'Save'),
                      );
                    },
                  ),
                  const SizedBox(height: 4),
                  FilledButton.tonalIcon(
                    onPressed: _addToPlaylist,
                    icon: const Icon(Icons.queue_music),
                    label: const Text('Add to playlist'),
                  ),
                  const SizedBox(height: 4),
                  FilledButton.tonalIcon(
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
            ],
          ),
          const SizedBox(height: 16),
          // Interactive SoundCloud Waveform Scrubber
          Card(
            color: Theme.of(context).colorScheme.surfaceContainerHighest,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      IconButton.filled(
                        icon: Icon(
                          trackPlaying ? Icons.pause : Icons.play_arrow,
                        ),
                        onPressed: () {
                          final shell = context.read<ShellService>();
                          if (shell.audioPlaying &&
                              shell.audioTitle == _track.title) {
                            shell.stopAudio();
                          } else {
                            shell.playAudio(_track.audioUrl, _track.title);
                          }
                        },
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(_track.title,
                                style: const TextStyle(
                                    fontWeight: FontWeight.bold),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis),
                            Text(
                                '${_playbackPosition.toStringAsFixed(1)}s / ${_trackDuration.toStringAsFixed(1)}s',
                                style: Theme.of(context).textTheme.bodySmall),
                          ],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),
                  // Waveform Bar Scrubber
                  GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onHorizontalDragUpdate: (details) {
                      final box = context.findRenderObject() as RenderBox?;
                      if (box != null) {
                        final local =
                            details.localPosition.dx.clamp(0.0, box.size.width);
                        setState(() {
                          _playbackPosition =
                              (local / box.size.width) * _trackDuration;
                        });
                      }
                    },
                    child: SizedBox(
                      height: 56,
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.end,
                        children: [
                          for (int i = 0; i < 40; i++) ...[
                            Expanded(
                              child: Container(
                                height: 12.0 + ((i * 7 + 13) % 40).toDouble(),
                                margin:
                                    const EdgeInsets.symmetric(horizontal: 1),
                                decoration: BoxDecoration(
                                  color: (i / 40.0) <=
                                          (_playbackPosition / _trackDuration)
                                      ? Colors.orangeAccent
                                      : Colors.grey.shade400,
                                  borderRadius: BorderRadius.circular(2),
                                ),
                              ),
                            ),
                          ],
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(height: 6),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text('0:00',
                          style: Theme.of(context).textTheme.bodySmall),
                      Text(
                          'Comment at ${_playbackPosition.toStringAsFixed(0)}s',
                          style: TextStyle(
                              fontSize: 11,
                              color: Theme.of(context).colorScheme.primary,
                              fontWeight: FontWeight.bold)),
                      Text(
                          '${(_trackDuration / 60).floor()}:${(_trackDuration % 60).floor().toString().padLeft(2, '0')}',
                          style: Theme.of(context).textTheme.bodySmall),
                    ],
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 16),
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
