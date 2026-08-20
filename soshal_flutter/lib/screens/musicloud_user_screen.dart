import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../services/media_service.dart';
import '../services/music_service.dart';
import '../services/p2p_service.dart';
import '../services/shell_service.dart';
import '../utils/format.dart';
import '../widgets/blob_image.dart';
import '../widgets/user_content_list.dart';

/// Musicloud user page: one author's published tracks (kind 31022).
class MusicloudUserScreen extends StatefulWidget {
  /// Author pubkey being listed.
  final String pubkey;

  /// Musicloud user screen.
  const MusicloudUserScreen({super.key, required this.pubkey});

  @override
  State<MusicloudUserScreen> createState() => _MusicloudUserScreenState();
}

class _MusicloudUserScreenState extends State<MusicloudUserScreen> {
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      await context.read<MusicService>().fetchTracks(author: widget.pubkey);
    } catch (e) {
      debugPrint('musicloud user load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  void _openTrack(MusicTrack track) {
    showModalBottomSheet<void>(
      context: context,
      builder: (sheetContext) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                track.title.isEmpty ? 'Untitled track' : track.title,
                style: Theme.of(sheetContext).textTheme.titleLarge,
              ),
              const SizedBox(height: 4),
              Text(
                '${shortPubkey(track.pubkey)} · ${relativeTime(track.createdAt)}',
                style: Theme.of(sheetContext).textTheme.bodySmall,
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: () async {
                  final shell = context.read<ShellService>();
                  final url = await resolveTrackPlaybackUrl(
                    track,
                    context.read<MediaService>(),
                    context.read<P2pService>(),
                  );
                  if (url == null) {
                    if (!sheetContext.mounted) return;
                    ScaffoldMessenger.of(sheetContext).showSnackBar(
                      const SnackBar(
                          content: Text(
                              'Audio unavailable — no device has this blob.')),
                    );
                    return;
                  }
                  final ok = await shell.playAudio(url, track.title);
                  if (!sheetContext.mounted) return;
                  if (!ok) {
                    ScaffoldMessenger.of(sheetContext).showSnackBar(
                      const SnackBar(
                          content: Text(
                              'Playback failed — check the audio output and try again.')),
                    );
                    return;
                  }
                  Navigator.of(sheetContext).pop();
                },
                icon: const Icon(Icons.play_circle_outline),
                label: const Text('Play'),
              ),
              const SizedBox(height: 8),
              FilledButton.tonalIcon(
                onPressed: () {
                  Clipboard.setData(ClipboardData(text: track.audioUrl));
                  Navigator.of(sheetContext).pop();
                  if (!context.mounted) return;
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                        content: Text('Audio URL copied to clipboard')),
                  );
                },
                icon: const Icon(Icons.copy),
                label: const Text('Copy audio URL'),
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final tracks = context.select((MusicService s) => s.tracks);
    final error = context.select((MusicService s) => s.lastError);
    return UserContentList(
      title: 'Music',
      loading: _loading,
      onRefresh: _load,
      error: error,
      emptyIcon: Icons.music_note,
      emptyTitle: 'No tracks published',
      emptyBody: 'This author has not published any Musicloud tracks yet.',
      itemCount: tracks.length,
      itemBuilder: (context, index) {
        final track = tracks[index];
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
          trailing: IconButton(
            icon: const Icon(Icons.open_in_new),
            tooltip: 'Track URL',
            onPressed: () => _openTrack(track),
          ),
          onTap: () => _openTrack(track),
        );
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
